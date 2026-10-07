// SPDX-License-Identifier: Apache-2.0
// rvtest_config.h: what the Vreteno core implements, in the macros the
// architectural tests read.
//
// The ACT4 framework writes this file from a UDB description of the
// core, with Ruby tools this build does not use. It is written here by
// hand instead, from the core's own source in TxHDL
// (`cpu/vreteno/src/isa.rs` and `cpu/vreteno/src/core.rs`), and it
// must agree with `sail.json` beside it and with `//act:defs.bzl`,
// which selects the tests.

#ifndef RVTEST_CONFIG_H
#define RVTEST_CONFIG_H

// RV32, misa = I, M, A, C, S, U.
#define UDB_MXLEN 32
#define I_SUPPORTED
#define M_SUPPORTED
#define ZMMUL_SUPPORTED
#define A_SUPPORTED
#define ZAAMO_SUPPORTED
#define ZALRSC_SUPPORTED
#define C_SUPPORTED
#define ZCA_SUPPORTED
#define ZICSR_SUPPORTED
#define ZIFENCEI_SUPPORTED
#define ZICNTR_SUPPORTED

// Machine, supervisor and user modes, and Sv32. The core has no
// menvcfg and no senvcfg, which privileged version 1.12 requires once
// user mode exists, so it is described as version 1.11.
#define SM_SUPPORTED
#define SM1P11P0_SUPPORTED
#define S_SUPPORTED
#define S1P11P0_SUPPORTED
#define U_SUPPORTED
#define SV32_SUPPORTED

// The time CSR reads the core's `time` input; no emulation is needed.
#define UDB_TIME_CSR_IMPLEMENTED

// mtvec and stvec are direct only, four-byte aligned.
#define UDB_MTVEC_BASE_ALIGNMENT_DIRECT 4
#define UDB_STVEC_BASE_ALIGNMENT_DIRECT 4

// No physical memory protection, no mcountinhibit, no hardware
// performance counters.
#define UDB_NUM_PMP_ENTRIES 0
#define UDB_NUM_USABLE_PMP_ENTRIES 0

#endif // RVTEST_CONFIG_H
