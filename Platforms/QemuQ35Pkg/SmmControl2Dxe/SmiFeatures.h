/**@file
  Negotiate SMI features with QEMU, and configure UefiCpuPkg/PiSmmCpuDxeSmm
  accordingly.

  Copyright (C) 2016-2017, Red Hat, Inc.
  Copyright (c) Microsoft Corporation

  SPDX-License-Identifier: BSD-2-Clause-Patent
**/

#ifndef __SMI_FEATURES_H__
#define __SMI_FEATURES_H__

/**
  Negotiate SMI features with QEMU.

  This function does not return a value. It simply figures out if SMI feature
  negotiation is available and compatible with our requirements. If
  incompatible, this function hangs.
**/
VOID
NegotiateSmiFeatures (
  VOID
  );

#endif
