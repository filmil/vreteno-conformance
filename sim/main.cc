// SPDX-License-Identifier: Apache-2.0
//
// The testbench: runs one test ELF on the Vreteno netlist under
// Verilator.
//
// The netlist's only path to memory is its AXI4 manager port. This
// program answers that port with a memory, loads the ELF into the
// memory, and holds the debug and interrupt inputs at zero.
//
// A test ends the way it ends on the Sail reference model, through the
// HTIF `tohost` word, whose address is read from the ELF's symbol
// table. A write to the upper half of `tohost` completes a command:
//
//   * device 1, command 1 (`0x0101` in the upper 16 bits) prints the
//     low byte of the lower half on the console;
//   * otherwise, an odd lower half ends the test, with exit code
//     `lower >> 1`. Zero is a pass.
//
// Usage:
//   sim --elf <test.elf> --out <result file> [--name <test name>]
//       [--max-cycles <n>]
//
// The result file is one line of `key=value` pairs:
//   name=<test> status=PASS|FAIL|TIMEOUT cycles=<n> retired=<n>
//   code=<exit code>
// followed by the console output. The program's exit status is zero
// whenever it wrote the result file, so that a failing test is a
// result to report, and not a broken build. The tests read the file.

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

#include "Vdut.h"
#include "verilated.h"

double sc_time_stamp() { return 0; }

namespace {

// A sparse byte-addressed memory, a 4 KiB page at a time. Unwritten
// bytes read as zero.
class Memory {
 public:
  uint8_t Read8(uint32_t addr) const {
    auto it = pages_.find(addr >> 12);
    if (it == pages_.end()) return 0;
    return it->second[addr & 0xfff];
  }
  void Write8(uint32_t addr, uint8_t v) { Page(addr)[addr & 0xfff] = v; }
  uint32_t Read32(uint32_t addr) const {
    uint32_t v = 0;
    for (int i = 0; i < 4; i++) v |= uint32_t(Read8(addr + i)) << (8 * i);
    return v;
  }
  void Write32(uint32_t addr, uint32_t v, uint32_t strobe = 0xf) {
    for (int i = 0; i < 4; i++) {
      if (strobe & (1u << i)) Write8(addr + i, uint8_t(v >> (8 * i)));
    }
  }

 private:
  std::vector<uint8_t>& Page(uint32_t addr) {
    auto& p = pages_[addr >> 12];
    if (p.empty()) p.resize(4096, 0);
    return p;
  }
  std::unordered_map<uint32_t, std::vector<uint8_t>> pages_;
};

// The parts of an ELF32 file this program reads.
struct Elf32Header {
  uint8_t ident[16];
  uint16_t type, machine;
  uint32_t version, entry, phoff, shoff, flags;
  uint16_t ehsize, phentsize, phnum, shentsize, shnum, shstrndx;
};
struct Elf32Phdr {
  uint32_t type, offset, vaddr, paddr, filesz, memsz, flags, align;
};
struct Elf32Shdr {
  uint32_t name, type, flags, addr, offset, size, link, info, addralign,
      entsize;
};
struct Elf32Sym {
  uint32_t name, value, size;
  uint8_t info, other;
  uint16_t shndx;
};

// Loads every loadable segment of `path` into `mem`, and returns the
// address of the symbol `tohost`, or zero if there is none.
uint32_t LoadElf(const std::string& path, Memory* mem) {
  std::ifstream f(path, std::ios::binary);
  if (!f) {
    std::cerr << "cannot open " << path << "\n";
    std::exit(2);
  }
  std::vector<char> data((std::istreambuf_iterator<char>(f)),
                         std::istreambuf_iterator<char>());
  if (data.size() < sizeof(Elf32Header) ||
      std::memcmp(data.data(), "\x7f" "ELF", 4) != 0 || data[4] != 1) {
    std::cerr << path << " is not an ELF32 file\n";
    std::exit(2);
  }
  Elf32Header eh;
  std::memcpy(&eh, data.data(), sizeof eh);
  for (int i = 0; i < eh.phnum; i++) {
    Elf32Phdr ph;
    std::memcpy(&ph, data.data() + eh.phoff + i * eh.phentsize, sizeof ph);
    if (ph.type != 1) continue;  // PT_LOAD
    for (uint32_t j = 0; j < ph.filesz; j++) {
      mem->Write8(ph.paddr + j, uint8_t(data[ph.offset + j]));
    }
  }
  uint32_t tohost = 0;
  for (int i = 0; i < eh.shnum; i++) {
    Elf32Shdr sh;
    std::memcpy(&sh, data.data() + eh.shoff + i * eh.shentsize, sizeof sh);
    if (sh.type != 2) continue;  // SHT_SYMTAB
    Elf32Shdr strtab;
    std::memcpy(&strtab, data.data() + eh.shoff + sh.link * eh.shentsize,
                sizeof strtab);
    for (uint32_t off = 0; off + sizeof(Elf32Sym) <= sh.size;
         off += sizeof(Elf32Sym)) {
      Elf32Sym sym;
      std::memcpy(&sym, data.data() + sh.offset + off, sizeof sym);
      const char* name = data.data() + strtab.offset + sym.name;
      if (std::strcmp(name, "tohost") == 0) tohost = sym.value;
    }
  }
  return tohost;
}

// A burst accepted on the read or the write address channel.
struct Burst {
  uint32_t id, addr, len, size, burst;
};

// The address of beat `beat` of `b`, as AXI4 computes it.
uint32_t BeatAddr(const Burst& b, uint32_t beat) {
  uint32_t bytes = 1u << b.size;
  uint32_t aligned = b.addr & ~(bytes - 1);
  switch (b.burst) {
    case 0:  // FIXED
      return b.addr;
    case 2: {  // WRAP
      uint32_t total = bytes * (b.len + 1);
      uint32_t base = b.addr & ~(total - 1);
      return base + ((b.addr - base + beat * bytes) % total);
    }
    default:  // INCR
      return (beat == 0 ? b.addr : aligned + beat * bytes);
  }
}

}  // namespace

int main(int argc, char** argv) {
  std::string elf, out, name = "test";
  uint64_t max_cycles = 20'000'000;
  for (int i = 1; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() -> std::string {
      if (i + 1 >= argc) {
        std::cerr << a << " needs a value\n";
        std::exit(2);
      }
      return argv[++i];
    };
    if (a == "--elf") elf = next();
    else if (a == "--out") out = next();
    else if (a == "--name") name = next();
    else if (a == "--max-cycles") max_cycles = std::stoull(next());
  }
  if (elf.empty() || out.empty()) {
    std::cerr << "usage: sim --elf <elf> --out <result> [--name <name>]"
                 " [--max-cycles <n>]\n";
    return 2;
  }

  Memory mem;
  const uint32_t tohost = LoadElf(elf, &mem);
  if (tohost == 0) {
    std::cerr << elf << " has no tohost symbol\n";
    return 2;
  }

  auto ctx = std::make_unique<VerilatedContext>();
  auto dut = std::make_unique<Vdut>(ctx.get());

  // Every ready is high: the memory takes any address phase and any
  // write beat at once, and queues them.
  dut->mem_awready = 1;
  dut->mem_wready = 1;
  dut->mem_arready = 1;
  dut->mem_bvalid = 0;
  dut->mem_rvalid = 0;
  dut->mei = dut->mti = dut->msi = dut->sei = 0;
  dut->haltreq = dut->resumereq = dut->dbg_we = 0;
  dut->dbg_regno = 0;
  dut->dbg_wdata = 0;
  dut->time_rw = 0;
  dut->rst = 1;
  dut->clk = 0;
  dut->eval();

  std::deque<Burst> reads;    // read bursts, in order
  uint32_t read_beat = 0;     // the next beat of reads.front()
  std::deque<Burst> writes;   // write bursts waiting for beats
  uint32_t write_beat = 0;    // the next beat of writes.front()
  std::deque<uint32_t> bids;  // write responses to send
  // Beats that arrived before their address phase.
  std::deque<std::pair<uint32_t, uint32_t>> early_beats;

  std::string console;
  std::string status = "TIMEOUT";
  uint32_t code = 0;
  uint64_t cycle = 0, retired = 0;
  const uint64_t reset_cycles = 8;

  auto write_word = [&](uint32_t addr, uint32_t data, uint32_t strb) {
    mem.Write32(addr & ~3u, data, strb);
    // HTIF: a write to the upper half of tohost completes a command.
    if ((addr & ~3u) == tohost + 4) {
      uint32_t hi = mem.Read32(tohost + 4);
      uint32_t lo = mem.Read32(tohost);
      if ((hi >> 16) == 0x0101) {
        console.push_back(char(lo & 0xff));
      } else if (lo & 1) {
        code = lo >> 1;
        status = (code == 0) ? "PASS" : "FAIL";
      }
      mem.Write32(tohost, 0);
      mem.Write32(tohost + 4, 0);
    }
  };

  for (cycle = 0; cycle < max_cycles && status == "TIMEOUT"; cycle++) {
    dut->rst = cycle < reset_cycles;
    dut->time_rw = cycle;
    dut->clk = 0;
    dut->eval();

    // What is handshaken at this rising edge.
    const bool aw_fire = dut->awvalid && dut->mem_awready;
    const bool w_fire = dut->wvalid && dut->mem_wready;
    const bool ar_fire = dut->arvalid && dut->mem_arready;
    const bool b_fire = dut->mem_bvalid && dut->bready;
    const bool r_fire = dut->mem_rvalid && dut->rready;
    const Burst aw{dut->awid, dut->awaddr, dut->awlen, dut->awsize,
                   dut->awburst};
    const Burst ar{dut->arid, dut->araddr, dut->arlen, dut->arsize,
                   dut->arburst};
    const uint32_t wdata = dut->wdata, wstrb = dut->wstrb;
    if (!dut->rst && (dut->retire >> 37) & 1) retired++;

    dut->clk = 1;
    dut->eval();

    // The memory's side of the edge.
    if (aw_fire) writes.push_back(aw);
    if (w_fire) early_beats.emplace_back(wdata, wstrb);
    while (!writes.empty() && !early_beats.empty()) {
      const Burst& b = writes.front();
      auto [d, s] = early_beats.front();
      early_beats.pop_front();
      write_word(BeatAddr(b, write_beat), d, s);
      if (write_beat == b.len) {
        bids.push_back(b.id);
        writes.pop_front();
        write_beat = 0;
      } else {
        write_beat++;
      }
    }
    if (b_fire) bids.pop_front();
    if (ar_fire) reads.push_back(ar);
    if (r_fire) {
      if (read_beat == reads.front().len) {
        reads.pop_front();
        read_beat = 0;
      } else {
        read_beat++;
      }
    }

    // What the memory offers for the next edge.
    dut->mem_bvalid = !bids.empty();
    dut->mem_bid = bids.empty() ? 0 : bids.front();
    dut->mem_bresp = 0;
    dut->mem_rvalid = !reads.empty();
    if (!reads.empty()) {
      const Burst& b = reads.front();
      dut->mem_rid = b.id;
      dut->mem_rdata = mem.Read32(BeatAddr(b, read_beat) & ~3u);
      dut->mem_rresp = 0;
      dut->mem_rlast = read_beat == b.len;
    }
  }
  dut->final();

  std::ofstream o(out);
  o << "name=" << name << " status=" << status << " cycles=" << cycle
    << " retired=" << retired << " code=" << code << "\n";
  o << console;
  if (!console.empty() && console.back() != '\n') o << "\n";
  o.close();
  std::cout << name << ": " << status << " after " << cycle << " cycles, "
            << retired << " instructions\n";
  return 0;
}
