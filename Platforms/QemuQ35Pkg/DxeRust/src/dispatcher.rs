use alloc::{
  collections::{BTreeSet, VecDeque},
  vec::Vec,
};
use core::ffi::c_void;
use r_efi::{efi, system::TPL_CALLBACK};
use r_pi::fw_fs::{ffs, FfsFileRawType, FfsSectionType, FirmwareVolume, FirmwareVolumeBlockProtocol};
use uefi_depex_lib::{Depex, Opcode};

use crate::{
  events::{raise_tpl, restore_tpl, EVENT_DB},
  image::{core_load_image, get_dxe_core_handle, start_image},
  println,
  protocols::PROTOCOL_DB,
};

// Default Dependency expression per PI spec v1.2 Vol 2 section 10.9.
const DEFAULT_DEPEX: &[Opcode] = &[
  Opcode::Push(Some(uuid::Uuid::from_u128(0x665e3ff6_46cc_11d4_9a38_0090273fc14d))), //BDS Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb1_6f42_11d4_bce7_0080c73c8881))), //Cpu Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb2_6f42_11d4_bce7_0080c73c8881))), //Metronome Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x1da97072_bddc_4b30_99f1_72a0b56fff2a))), //Monotonic Counter Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x27cfac87_46cc_11d4_9a38_0090273fc14d))), //Real Time Clock Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x27cfac88_46cc_11d4_9a38_0090273fc14d))), //Reset Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0xb7dfb4e1_052f_449f_87be_9818fc91b733))), //Runtime Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0xa46423e3_4617_49f1_b9ff_d1bfa9115839))), //Security Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x26baccb3_6f42_11d4_bce7_0080c73c8881))), //Timer Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x6441f818_6362_4e44_b570_7dba31dd2453))), //Variable Write Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x1e5668e2_8481_11d4_bcf1_0080c73c8881))), //Variable Arch
  Opcode::Push(Some(uuid::Uuid::from_u128(0x665e3ff5_46cc_11d4_9a38_0090273fc14d))), //Watchdog Arch
  Opcode::And,                                                                       //Variable + Watchdog
  Opcode::And,                                                                       //+Variable Write
  Opcode::And,                                                                       //+Timer
  Opcode::And,                                                                       //+Security
  Opcode::And,                                                                       //+Runtime
  Opcode::And,                                                                       //+Reset
  Opcode::And,                                                                       //+Real Time Clock
  Opcode::And,                                                                       //+Monotonic Counter
  Opcode::And,                                                                       //+Metronome
  Opcode::And,                                                                       //+Cpu
  Opcode::And,                                                                       //+Bds
  Opcode::End,
];

#[derive(Debug)]
struct ScheduledDriver {
  file: ffs::File,
  device_path: *mut efi::protocols::device_path::Protocol,
  execution_attempted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FvInfo {
  base_address: u64,
  handle: efi::Handle,
  device_path: *mut efi::protocols::device_path::Protocol,
}

#[derive(Debug)]
struct DispatcherContext {
  discovered_fv_info: spin::Mutex<BTreeSet<FvInfo>>, //anything interacting with this has to run at TPL_CALLBACK.
  processed_fvs: spin::Mutex<BTreeSet<efi::Handle>>, //anything interacting with this has to run at TPL_CALLBACK.
  scheduled_driver_base_addresses: spin::Mutex<VecDeque<ScheduledDriver>>, //only used in dispatch loop, should not be touched from anywhere else.
}

impl DispatcherContext {
  const fn new() -> Self {
    Self {
      discovered_fv_info: spin::Mutex::new(BTreeSet::new()),
      processed_fvs: spin::Mutex::new(BTreeSet::new()),
      scheduled_driver_base_addresses: spin::Mutex::new(VecDeque::new()),
    }
  }

  fn add_fv_handles(&self, new_handles: Vec<efi::Handle>) {
    let mut fv_handles = self.processed_fvs.lock();
    for handle in new_handles {
      if fv_handles.insert(handle) {
        //process freshly discovered FV
        let fvb_ptr = match PROTOCOL_DB.get_interface_for_handle(handle, FirmwareVolumeBlockProtocol::GUID) {
          Err(_) => {
            panic!("get_interface_for_handle failed to return an interface on a handle where it should have existed")
          }
          Ok(protocol) => protocol as *mut FirmwareVolumeBlockProtocol::Protocol,
        };

        let fvb =
          unsafe { fvb_ptr.as_ref().expect("get_interface_for_handle returned NULL ptr for FirmwareVolumeBlock") };

        let mut fv_address: u64 = 0;
        let status = (fvb.get_physical_address)(fvb_ptr, core::ptr::addr_of_mut!(fv_address));
        if status.is_error() {
          return;
        }

        let fv_device_path = PROTOCOL_DB.get_interface_for_handle(handle, efi::protocols::device_path::PROTOCOL_GUID);

        let fv_info = FvInfo {
          base_address: fv_address,
          handle: handle,
          device_path: fv_device_path.unwrap_or(core::ptr::null_mut()) as *mut efi::protocols::device_path::Protocol,
        };

        self.discovered_fv_info.lock().insert(fv_info);
      }
    }
  }

  fn evaluate_depex(file: ffs::File) -> bool {
    let depex_section = file.ffs_sections().find_map(|x| match x.section_type() {
      Some(FfsSectionType::DxeDepex) => {
        let data = x.section_data().to_vec();
        Some(data)
      }
      _ => None,
    });

    let depex = match depex_section {
      Some(depex) => Depex::from(depex),
      None => Depex::from(DEFAULT_DEPEX), //if no depex section, use default.
    };

    //print!("evaluating depex for {:?}", uuid::Uuid::from_bytes_le(*file.file_name().as_bytes()));
    let result = depex.eval(&PROTOCOL_DB);
    //println!(" result {:?}", result);
    //println!("full depex: {:#x?}", depex);
    result
  }

  fn dispatch(&self) -> bool {
    let mut dispatch_attempted = false;
    let old_tpl = raise_tpl(TPL_CALLBACK);
    let discovered_fv_info: Vec<FvInfo> = self.discovered_fv_info.lock().clone().into_iter().collect();
    restore_tpl(old_tpl);

    for fv_info in discovered_fv_info {
      let fv_base_address = fv_info.base_address;
      let fv = FirmwareVolume::new(fv_base_address);
      for file in fv.ffs_files() {
        match file.file_type_raw() {
          FfsFileRawType::DRIVER => {
            let mut scheduled_queue = self.scheduled_driver_base_addresses.lock();

            if scheduled_queue.iter().find(|x| x.file.base_address() == file.base_address()).is_none()
              && Self::evaluate_depex(file)
            {
              //depex is met, insert into scheduled queue
              scheduled_queue.push_back(ScheduledDriver {
                file: file.clone(),
                device_path: fv_info.device_path,
                execution_attempted: false,
              });
            }
          }
          _ => { /*don't care about other file types in the dispatcher */ }
        }
      }
    }

    for candidate in self.scheduled_driver_base_addresses.lock().iter_mut().filter(|x| x.execution_attempted == false) {
      println!("Evaluating candidate: {:?}", uuid::Uuid::from_bytes_le(*(candidate.file.file_name().as_bytes())));
      candidate.execution_attempted = true;

      let pe32_section = candidate.file.ffs_sections().find_map(|x| match x.section_type() {
        Some(FfsSectionType::Pe32) => Some(x.section_data().to_vec()),
        _ => None,
      });

      if let Some(pe32_data) = pe32_section {
        let image_load_result =
          core_load_image(get_dxe_core_handle(), candidate.device_path, Some(pe32_data.as_slice()));
        if let Ok(image_handle) = image_load_result {
          dispatch_attempted = true;
          let status = start_image(image_handle, core::ptr::null_mut(), core::ptr::null_mut());
          println!("Module Entry point finished with status: {:?}", status);
        } else {
          println!("Failed to load: load_image returned {:?}", image_load_result);
        }
      } else {
        println!("Failed to load: no PE32 section in candidate driver.");
      }
    }
    dispatch_attempted
  }
}

unsafe impl Sync for DispatcherContext {}
unsafe impl Send for DispatcherContext {}

static DISPATCHER_CONTEXT: DispatcherContext = DispatcherContext::new();

extern "efiapi" fn core_fw_vol_event_protocol_notify(_event: efi::Event, _context: *mut c_void) {
  //Note: runs at TPL_CALLBACK
  match PROTOCOL_DB.locate_handles(Some(FirmwareVolumeBlockProtocol::GUID)) {
    Ok(fv_handles) => DISPATCHER_CONTEXT.add_fv_handles(fv_handles),
    Err(_) => panic!("could not locate handles in protocol call back"),
  }
}

pub fn init_dispatcher() {
  //set up call back for FV protocol installation.
  let event = EVENT_DB
    .create_event(efi::EVT_NOTIFY_SIGNAL, efi::TPL_CALLBACK, Some(core_fw_vol_event_protocol_notify), None, None)
    .expect("Failed to create fv protocol installation callback.");

  PROTOCOL_DB
    .register_protocol_notify(FirmwareVolumeBlockProtocol::GUID, event)
    .expect("Failed to register protocol notify on fv protocol.");
}

pub fn core_dispatcher() {
  loop {
    let something_dispatched = DISPATCHER_CONTEXT.dispatch();
    if !something_dispatched {
      break;
    }
  }
}
