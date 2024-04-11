use core::ffi::c_void;

use alloc::{collections::BTreeSet, vec::Vec};
use mu_pi::{
  fw_fs::{ffs, FfsFileRawType, FfsSectionType, FirmwareVolume},
  protocols::firmware_volume_block,
};
use r_efi::efi;
use serial_print_dxe::println;
use tpl_lock::TplMutex;
use uefi_depex_lib::{Depex, Opcode};
use uefi_protocol_db_lib::DXE_CORE_HANDLE;

use crate::{
  events::EVENT_DB,
  image::{core_load_image, core_start_image},
  protocols::PROTOCOL_DB,
};

// Default Dependency expression per PI spec v1.2 Vol 2 section 10.9.
const ALL_ARCH_DEPEX: &[Opcode] = &[
  Opcode::Push(Some(uuid::Uuid::from_u128(0x665e3ff6_46cc_11d4_9a38_0090273fc14d)), false), //BDS Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb1_6f42_11d4_bce7_0080c73c8881)), false), //Cpu Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb2_6f42_11d4_bce7_0080c73c8881)), false), //Metronome Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x1da97072_bddc_4b30_99f1_72a0b56fff2a)), false), //Monotonic Counter Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x27cfac87_46cc_11d4_9a38_0090273fc14d)), false), //Real Time Clock Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x27cfac88_46cc_11d4_9a38_0090273fc14d)), false), //Reset Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0xb7dfb4e1_052f_449f_87be_9818fc91b733)), false), //Runtime Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0xa46423e3_4617_49f1_b9ff_d1bfa9115839)), false), //Security Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb3_6f42_11d4_bce7_0080c73c8881)), false), //Timer Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x6441f818_6362_4e44_b570_7dba31dd2453)), false), //Variable Write Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x1e5668e2_8481_11d4_bcf1_0080c73c8881)), false), //Variable Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x665e3ff5_46cc_11d4_9a38_0090273fc14d)), false), //Watchdog Arch
  Opcode::And,                                                                              //Variable + Watchdog
  Opcode::And,                                                                              //+Variable Write
  Opcode::And,                                                                              //+Timer
  Opcode::And,                                                                              //+Security
  Opcode::And,                                                                              //+Runtime
  Opcode::And,                                                                              //+Reset
  Opcode::And,                                                                              //+Real Time Clock
  Opcode::And,                                                                              //+Monotonic Counter
  Opcode::And,                                                                              //+Metronome
  Opcode::And,                                                                              //+Cpu
  Opcode::And,                                                                              //+Bds
  Opcode::End,
];

struct PendingDriver {
  file: ffs::File,
  device_path: *mut efi::protocols::device_path::Protocol,
  depex: Option<Depex>,
}

#[derive(Default)]
struct DispatcherContext {
  executing: bool,
  arch_protocols_available: bool,
  pending_drivers: Vec<PendingDriver>,
  processed_fvs: BTreeSet<efi::Handle>,
}

impl DispatcherContext {
  const fn new() -> Self {
    Self {
      executing: false,
      arch_protocols_available: false,
      pending_drivers: Vec::new(),
      processed_fvs: BTreeSet::new(),
    }
  }
}

unsafe impl Send for DispatcherContext {}

static DISPATCHER_CONTEXT: TplMutex<DispatcherContext> =
  TplMutex::new(efi::TPL_NOTIFY, DispatcherContext::new(), "Dispatcher Context");

fn dispatch() -> Result<bool, efi::Status> {
  println!("Evaluating depex");
  let mut scheduled = Vec::new();
  {
    let mut dispatcher = DISPATCHER_CONTEXT.lock();
    if !dispatcher.arch_protocols_available {
      dispatcher.arch_protocols_available = Depex::from(ALL_ARCH_DEPEX).eval(&PROTOCOL_DB);
    }
    let candidates: Vec<_> = dispatcher.pending_drivers.drain(..).collect();
    for mut candidate in candidates {
      let depex_satisfied = match candidate.depex {
        Some(ref mut depex) => depex.eval(&PROTOCOL_DB),
        None => dispatcher.arch_protocols_available,
      };

      if depex_satisfied {
        scheduled.push(candidate)
      } else {
        dispatcher.pending_drivers.push(candidate);
      }
    }
  }
  println!("Depex evaluation complete, scheduled {:} drivers", scheduled.len());

  let mut dispatch_attempted = false;
  for driver in scheduled {
    let pe32_section = driver.file.ffs_sections().find_map(|x| match x.section_type() {
      Some(FfsSectionType::Pe32) => Some(x.section_data().to_vec()),
      _ => None,
    });

    if let Some(pe32_data) = pe32_section {
      let image_load_result = core_load_image(false, DXE_CORE_HANDLE, driver.device_path, Some(pe32_data.as_slice()));
      if let Ok(image_handle) = image_load_result {
        dispatch_attempted = true;
        let status = match core_start_image(image_handle) {
          Ok(()) => efi::Status::SUCCESS,
          Err(err) => err,
        };
        println!("Module Entry point finished with status: {:?}", status);
      } else {
        println!("Failed to load: load_image returned {:?}", image_load_result);
      }
    } else {
      println!("Failed to load: no PE32 section in candidate driver.");
    }
  }

  Ok(dispatch_attempted)
}

fn add_fv_handles(new_handles: Vec<efi::Handle>) {
  let mut dispatcher = DISPATCHER_CONTEXT.lock();
  for handle in new_handles {
    if dispatcher.processed_fvs.insert(handle) {
      //process freshly discovered FV
      let fvb_ptr = match PROTOCOL_DB.get_interface_for_handle(handle, firmware_volume_block::PROTOCOL_GUID) {
        Err(_) => {
          panic!("get_interface_for_handle failed to return an interface on a handle where it should have existed")
        }
        Ok(protocol) => protocol as *mut firmware_volume_block::Protocol,
      };

      let fvb =
        unsafe { fvb_ptr.as_ref().expect("get_interface_for_handle returned NULL ptr for FirmwareVolumeBlock") };

      let mut fv_address: u64 = 0;
      let status = (fvb.get_physical_address)(fvb_ptr, core::ptr::addr_of_mut!(fv_address));
      if status.is_error() {
        return;
      }

      let fv_device_path = PROTOCOL_DB.get_interface_for_handle(handle, efi::protocols::device_path::PROTOCOL_GUID);
      let fv_device_path =
        fv_device_path.unwrap_or(core::ptr::null_mut()) as *mut efi::protocols::device_path::Protocol;

      let fv = FirmwareVolume::new(fv_address);
      for file in fv.ffs_files() {
        if file.file_type_raw() == FfsFileRawType::DRIVER {
          let depex_section = file.ffs_sections().find_map(|x| {
            if x.section_type() == Some(FfsSectionType::DxeDepex) {
              let data = x.section_data().to_vec();
              Some(data)
            } else {
              None
            }
          });
          let depex = depex_section.map(Depex::from);
          dispatcher.pending_drivers.push(PendingDriver { file, device_path: fv_device_path, depex });
        }
      }
    }
  }
}

pub fn core_dispatcher() -> Result<(), efi::Status> {
  if DISPATCHER_CONTEXT.lock().executing {
    return Err(efi::Status::ALREADY_STARTED);
  }

  let mut something_dispatched = false;
  while dispatch()? {
    something_dispatched = true;
  }

  if something_dispatched {
    Ok(())
  } else {
    Err(efi::Status::NOT_FOUND)
  }
}

pub fn init_dispatcher() {
  //set up call back for FV protocol installation.
  let event = EVENT_DB
    .create_event(efi::EVT_NOTIFY_SIGNAL, efi::TPL_CALLBACK, Some(core_fw_vol_event_protocol_notify), None, None)
    .expect("Failed to create fv protocol installation callback.");

  PROTOCOL_DB
    .register_protocol_notify(firmware_volume_block::PROTOCOL_GUID, event)
    .expect("Failed to register protocol notify on fv protocol.");
}

pub fn display_discovered_not_dispatched() {
  for driver in &DISPATCHER_CONTEXT.lock().pending_drivers {
    let file_name = uuid::Uuid::from_bytes_le(*driver.file.file_name().as_bytes());
    println!("Driver {:?} found but not dispatched.", file_name);
  }
}

extern "efiapi" fn core_fw_vol_event_protocol_notify(_event: efi::Event, _context: *mut c_void) {
  //Note: runs at TPL_CALLBACK
  match PROTOCOL_DB.locate_handles(Some(firmware_volume_block::PROTOCOL_GUID)) {
    Ok(fv_handles) => add_fv_handles(fv_handles),
    Err(_) => panic!("could not locate handles in protocol call back"),
  }
}
