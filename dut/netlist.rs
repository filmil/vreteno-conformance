// SPDX-License-Identifier: Apache-2.0
//! The Vreteno hart as a device under test, and its netlist.
//!
//! The unit `Dut` joins three parts from TxHDL: the hart, which is
//! the core and its memory management unit; the core's tracker,
//! which turns the core's transactions into the five AXI4 channels;
//! and the adapter that puts those channels on AXI4 pins.
//! So the netlist has one AXI4 manager port, and the testbench
//! answers it with a memory.
//! Every address the core reaches goes out on that port.
//!
//! The core fetches its first 4 KiB from a memory inside itself.
//! That memory holds a boot stub of two instructions, which jumps to
//! `0x4000_0000`, where the tests are linked. That is where the board
//! has its DDR3, and the core's instruction cache holds words from
//! there, so the tests run cached, as programs on the board do.
//! A program counter at or above 4 KiB fetches from the bus, so the
//! whole test runs from the testbench's memory.
//!
//! The binary prints the Verilog of `Dut` on standard output.
use txhdl::comp::{chan, join2, signal, DefaultClock, In, Out, Unit};
use txhdl::types::{Bit, U};
use txhdl::{lower, Trace};
use txhdl_parts::bus::axi::{Ar, Aw, AxiHost, Done, Grant, Issue, B, R, W};
use txhdl_parts::bus::axi_per_pins::{AxiPerDriven, AxiPerPins, AxiPerPinsIn, AxiPerPinsOut};
use vreteno32::core::Writeback;
use vreteno32::hart::Hart;

/// The identifier width of the link, as the board uses it.
const IW: usize = 2;

/// Where the tests start: the base of the testbench's memory.
const TEST_BASE: u32 = 0x4000_0000;

/// The boot stub: `lui t0, %hi(TEST_BASE)`, then `jr t0`.
const BOOT: [u32; 2] = [(TEST_BASE & 0xffff_f000) | 0x2b7, 0x0002_8067];

/// The inputs of the device under test.
pub struct DutIn {
    /// The reset, high for as long as the core is held.
    pub rst: In<Bit>,
    /// The machine external, timer and software interrupt lines.
    pub mei: In<Bit>,
    pub mti: In<Bit>,
    pub msi: In<Bit>,
    /// The supervisor external interrupt line.
    pub sei: In<Bit>,
    /// The value the `time` CSR reads.
    pub time: In<U<64>>,
    /// The debug module's lines into the core. The testbench holds
    /// them at zero.
    pub haltreq: In<Bit>,
    pub resumereq: In<Bit>,
    pub dbg_regno: In<U<16>>,
    pub dbg_wdata: In<U<32>>,
    pub dbg_we: In<Bit>,
    /// What the memory drives on the AXI4 port.
    pub mem: AxiPerDriven<32, IW>,
}

/// The outputs of the device under test.
pub struct DutOut {
    /// The core has halted.
    pub halt: Out<Bit>,
    /// The instruction in execute, for a trace.
    pub instr: Out<U<32>>,
    /// What the core retired this cycle, for a trace.
    pub retire: Out<Writeback>,
    /// The core is in debug mode, and the register it read.
    pub debug: Out<Bit>,
    pub dbg_rdata: Out<U<32>>,
    /// The AXI4 manager port, named as AXI4 names its signals.
    pub awid: Out<U<IW>>,
    pub awaddr: Out<U<32>>,
    pub awlen: Out<U<8>>,
    pub awsize: Out<U<3>>,
    pub awburst: Out<U<2>>,
    pub awlock: Out<Bit>,
    pub awcache: Out<U<4>>,
    pub awprot: Out<U<3>>,
    pub awqos: Out<U<4>>,
    pub awvalid: Out<Bit>,
    pub wdata: Out<U<32>>,
    pub wstrb: Out<U<4>>,
    pub wlast: Out<Bit>,
    pub wvalid: Out<Bit>,
    pub bready: Out<Bit>,
    pub arid: Out<U<IW>>,
    pub araddr: Out<U<32>>,
    pub arlen: Out<U<8>>,
    pub arsize: Out<U<3>>,
    pub arburst: Out<U<2>>,
    pub arlock: Out<Bit>,
    pub arcache: Out<U<4>>,
    pub arprot: Out<U<3>>,
    pub arqos: Out<U<4>>,
    pub arvalid: Out<Bit>,
    pub rready: Out<Bit>,
}

/// The device under test: the hart, its tracker and the pins.
#[derive(Trace, Default)]
pub struct Dut {
    pub cpu: Hart<IW>,
    pub host: AxiHost<32, 32, 4, IW, 4>,
    pub pins: AxiPerPins<32, 32, 4, IW>,
}

#[lower]
impl Unit for Dut {
    async fn run(
        &mut self,
        DutIn {
            rst,
            mei,
            mti,
            msi,
            sei,
            time,
            haltreq,
            resumereq,
            dbg_regno,
            dbg_wdata,
            dbg_we,
            mem,
        }: DutIn,
        DutOut {
            halt,
            instr,
            retire,
            debug,
            dbg_rdata,
            awid,
            awaddr,
            awlen,
            awsize,
            awburst,
            awlock,
            awcache,
            awprot,
            awqos,
            awvalid,
            wdata,
            wstrb,
            wlast,
            wvalid,
            bready,
            arid,
            araddr,
            arlen,
            arsize,
            arburst,
            arlock,
            arcache,
            arprot,
            arqos,
            arvalid,
            rready,
        }: DutOut,
    ) {
        // The core and its tracker.
        let (issue_tx, issue_rx) = chan::<Issue<32>, DefaultClock>();
        let (wbeat_tx, wbeat_rx) = chan::<W<32, 4>, DefaultClock>();
        let (release_tx, release_rx) = chan::<Grant<IW>, DefaultClock>();
        let (grant_tx, grant_rx) = chan::<Grant<IW>, DefaultClock>();
        let (done_tx, done_rx) = chan::<Done<IW>, DefaultClock>();
        let (rdata_tx, rdata_rx) = chan::<R<32, IW>, DefaultClock>();
        // The tracker and the pins.
        let (aw_tx, aw_rx) = chan::<Aw<32, IW>, DefaultClock>();
        let (ar_tx, ar_rx) = chan::<Ar<32, IW>, DefaultClock>();
        let (w_tx, w_rx) = chan::<W<32, 4>, DefaultClock>();
        let (b_tx, b_rx) = chan::<B<IW>, DefaultClock>();
        let (r_tx, r_rx) = chan::<R<32, IW>, DefaultClock>();
        // No other host writes the memory here, so the data cache's snoop
        // names no line (TxHDL issue 1275).
        let (_dc_snoop_out, dc_snoop) = signal::<U<9>, DefaultClock>();
        // Whether the core waits in wfi: no pin, since no test waits.
        let (asleep, _asleep) = signal::<Bit, DefaultClock>();
        // The memory port answers the design's reset itself (TxHDL issue
        // 1532).
        let pins_rst = rst.clone();
        join2(
            self.cpu.run(
                (
                    rst, mei, mti, msi, rdata_rx, done_rx, grant_rx, haltreq, resumereq, dbg_regno,
                    dbg_wdata, dbg_we, time, sei, dc_snoop,
                ),
                (
                    halt, instr, retire, issue_tx, wbeat_tx, release_tx, debug, dbg_rdata, asleep,
                ),
            ),
            join2(
                self.host.run(
                    (issue_rx, wbeat_rx, b_rx, r_rx, release_rx),
                    (aw_tx, ar_tx, w_tx, grant_tx, done_tx, rdata_tx),
                ),
                self.pins.run(
                    AxiPerPinsIn {
                        rst: pins_rst,
                        pins: mem,
                        aw: aw_rx,
                        ar: ar_rx,
                        w: w_rx,
                    },
                    AxiPerPinsOut {
                        b: b_tx,
                        r: r_tx,
                        awid,
                        awaddr,
                        awlen,
                        awsize,
                        awburst,
                        awlock,
                        awcache,
                        awprot,
                        awqos,
                        awvalid,
                        wdata,
                        wstrb,
                        wlast,
                        wvalid,
                        bready,
                        arid,
                        araddr,
                        arlen,
                        arsize,
                        arburst,
                        arlock,
                        arcache,
                        arprot,
                        arqos,
                        arvalid,
                        rready,
                    },
                ),
            ),
        )
        .await;
    }
}

fn main() {
    let mut net = Dut::lowered("dut");
    let boot: Vec<u128> = BOOT.iter().map(|&w| w as u128).collect();
    // The instruction memory is the core's, inside the hart, so it is
    // reached by hand, as TxHDL's own board netlist reaches it.
    for inst in &mut net.instances {
        if inst.name == "cpu" {
            for c in &mut inst.unit.instances {
                if c.name == "core" {
                    c.unit.init("imem", &boot);
                }
            }
        }
    }
    print!("{}", net.verilog());
}
