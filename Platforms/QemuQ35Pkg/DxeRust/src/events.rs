use core::{
  convert::TryFrom,
  ffi::c_void,
  sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering},
};

use alloc::vec;

use r_efi::efi;

use r_pi::{cpu_arch, timer};
use uefi_event_lib::{SpinLockedEventDb, TimerDelay};

use crate::protocols::PROTOCOL_DB;

pub static EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

static CURRENT_TPL: AtomicUsize = AtomicUsize::new(efi::TPL_APPLICATION);
static SYSTEM_TIME: AtomicU64 = AtomicU64::new(0);
static CPU_ARCH_PTR: AtomicPtr<cpu_arch::Protocol> = AtomicPtr::new(core::ptr::null_mut());
static EVENT_NOTIFIES_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

extern "efiapi" fn create_event(
  event_type: u32,
  notify_tpl: efi::Tpl,
  notify_function: Option<efi::EventNotify>,
  notify_context: *mut c_void,
  event: *mut efi::Event,
) -> efi::Status {
  if event.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  let notify_context = if !notify_context.is_null() { Some(notify_context) } else { None };

  let (event_type, event_group) = match event_type {
    efi::EVT_SIGNAL_EXIT_BOOT_SERVICES => (efi::EVT_NOTIFY_SIGNAL, Some(efi::EVENT_GROUP_EXIT_BOOT_SERVICES)),
    efi::EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE => (efi::EVT_NOTIFY_SIGNAL, Some(efi::EVENT_GROUP_VIRTUAL_ADDRESS_CHANGE)),
    other => (other, None),
  };

  match EVENT_DB.create_event(event_type, notify_tpl, notify_function, notify_context, event_group) {
    Ok(new_event) => {
      unsafe { *event = new_event };
      efi::Status::SUCCESS
    }
    Err(err) => err,
  }
}

extern "efiapi" fn create_event_ex(
  event_type: u32,
  notify_tpl: efi::Tpl,
  notify_function: Option<efi::EventNotify>,
  notify_context: *const c_void,
  event_group: *const efi::Guid,
  event: *mut efi::Event,
) -> efi::Status {
  if event.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  let notify_context = if !notify_context.is_null() { Some(notify_context as *mut c_void) } else { None };

  match event_type {
    efi::EVT_SIGNAL_EXIT_BOOT_SERVICES | efi::EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE => {
      return efi::Status::INVALID_PARAMETER
    }
    _ => (),
  }

  let event_group = if !event_group.is_null() { Some(unsafe { *event_group }) } else { None };

  match EVENT_DB.create_event(event_type, notify_tpl, notify_function, notify_context, event_group) {
    Ok(new_event) => {
      unsafe { *event = new_event };
      efi::Status::SUCCESS
    }
    Err(err) => err,
  }
}

pub extern "efiapi" fn close_event(event: efi::Event) -> efi::Status {
  match EVENT_DB.close_event(event) {
    Ok(()) => efi::Status::SUCCESS,
    Err(err) => err,
  }
}

pub extern "efiapi" fn signal_event(event: efi::Event) -> efi::Status {
  let status = match EVENT_DB.signal_event(event) {
    Ok(()) => efi::Status::SUCCESS,
    Err(err) => err,
  };

  //Note: The C-reference implementation of SignalEvent gets an immediate dispatch of
  //pending events as a side effect of the locking implementation calling raise/restore
  //TPL. The spec doesn't require this; but it's likely that code out there depends
  //on it. So emulate that here with an artificial raise/restore.
  let old_tpl = raise_tpl(efi::TPL_HIGH_LEVEL);
  restore_tpl(old_tpl);

  status
}

extern "efiapi" fn wait_for_event(
  number_of_events: usize,
  event_array: *mut efi::Event,
  out_index: *mut usize,
) -> efi::Status {
  if number_of_events == 0 || event_array.is_null() || out_index.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  if CURRENT_TPL.load(Ordering::SeqCst) != efi::TPL_APPLICATION {
    return efi::Status::UNSUPPORTED;
  }

  //get the events list as a slice
  let event_list = unsafe { core::slice::from_raw_parts(event_array, number_of_events) };

  //spin on the list
  loop {
    for (index, event) in event_list.iter().enumerate() {
      match check_event(*event) {
        efi::Status::NOT_READY => (),
        status => {
          unsafe { *out_index = index };
          return status;
        }
      }
    }
  }
}

pub extern "efiapi" fn check_event(event: efi::Event) -> efi::Status {
  let event_type = match EVENT_DB.get_event_type(event) {
    Ok(event_type) => event_type,
    Err(err) => return err,
  };

  if event_type.is_notify_signal() {
    return efi::Status::INVALID_PARAMETER;
  }

  match EVENT_DB.read_and_clear_signalled(event) {
    Ok(signalled) => {
      if signalled {
        return efi::Status::SUCCESS;
      }
    }
    Err(err) => return err,
  }

  match EVENT_DB.queue_event_notify(event) {
    Ok(()) => (),
    Err(err) => return err,
  }

  // raise/restore TPL to allow notifies to occur at the appropriate level.
  let old_tpl = raise_tpl(efi::TPL_HIGH_LEVEL);
  restore_tpl(old_tpl);

  match EVENT_DB.read_and_clear_signalled(event) {
    Ok(signalled) => {
      if signalled {
        return efi::Status::SUCCESS;
      }
    }
    Err(err) => return err,
  }

  efi::Status::NOT_READY
}

pub extern "efiapi" fn set_timer(event: efi::Event, timer_type: efi::TimerDelay, trigger_time: u64) -> efi::Status {
  let timer_type = match TimerDelay::try_from(timer_type) {
    Err(err) => return err,
    Ok(timer_type) => timer_type,
  };

  let (trigger_time, period) = match timer_type {
    TimerDelay::TimerCancel => (None, None),
    TimerDelay::TimerRelative => (Some(SYSTEM_TIME.load(Ordering::SeqCst) + trigger_time), None),
    TimerDelay::TimerPeriodic => (Some(SYSTEM_TIME.load(Ordering::SeqCst) + trigger_time), Some(trigger_time)),
  };

  match EVENT_DB.set_timer(event, timer_type, trigger_time, period) {
    Ok(()) => efi::Status::SUCCESS,
    Err(err) => err,
  }
}

pub extern "efiapi" fn raise_tpl(new_tpl: efi::Tpl) -> efi::Tpl {
  assert!(new_tpl <= efi::TPL_HIGH_LEVEL, "Invalid attempt to raise TPL above TPL_HIGH_LEVEL");

  let prev_tpl = CURRENT_TPL.fetch_max(new_tpl, Ordering::SeqCst);

  assert!(
    new_tpl >= prev_tpl,
    "Invalid attempt to raise TPL to lower value. New TPL: {:#x?}, Prev TPL: {:#x?}",
    new_tpl,
    prev_tpl
  );

  if (new_tpl == efi::TPL_HIGH_LEVEL) && (prev_tpl < efi::TPL_HIGH_LEVEL) {
    set_interrupt_state(false);
  }
  prev_tpl
}

pub extern "efiapi" fn restore_tpl(new_tpl: efi::Tpl) {
  let prev_tpl = CURRENT_TPL.fetch_min(new_tpl, Ordering::SeqCst);

  assert!(
    new_tpl <= prev_tpl,
    "Invalid attempt to restore TPL to higher value. New TPL: {:#x?}, Prev TPL: {:#x?}",
    new_tpl,
    prev_tpl
  );

  if new_tpl < prev_tpl {
    // Care must be taken to deal with re-entrant "restore_tpl" cases. For example, the event_notification_iter created
    // here requires taking the lock on EVENT_DB to iterate. The release of that lock will call restore_tpl.
    // To avoid infinite recursion, this logic uses EVENT_NOTIFIES_IN_PROGRESS to ensure that only one instance of
    // restore_tpl is accessing the locked EVENT_DB. restore_tpl calls that occur while the event notification iter is
    // in use will get back an empty vector of event notifications and will simply restore the TPL and exit.
    let events = match EVENT_NOTIFIES_IN_PROGRESS.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed) {
      Ok(_) => {
        let events = EVENT_DB.event_notification_iter(new_tpl).collect();
        EVENT_NOTIFIES_IN_PROGRESS.store(false, Ordering::Release);
        events
      }
      Err(_) => vec![],
    };

    for event in events {
      if event.notify_tpl < efi::TPL_HIGH_LEVEL {
        set_interrupt_state(true);
      } else {
        set_interrupt_state(false);
      }
      CURRENT_TPL.store(event.notify_tpl, Ordering::SeqCst);
      let notify_context = match event.notify_context {
        Some(context) => context,
        None => core::ptr::null_mut(),
      };

      //Caution: this is calling function pointer supplied by code outside DxeRust.
      //The notify_function is not "unsafe" per the signature, even though it's
      //supplied by code outside the dxe_rust module. If it were marked 'unsafe'
      //then other Rust modules executing under DxeRust would need to mark all event
      //callbacks as "unsafe", and the r_efi definition for EventNotify would need to
      //change.
      (event.notify_function)(event.event, notify_context);
    }
  }

  if new_tpl < efi::TPL_HIGH_LEVEL {
    set_interrupt_state(true);
  }
  CURRENT_TPL.store(new_tpl, Ordering::SeqCst);
}

extern "efiapi" fn timer_tick(time: u64) {
  let old_tpl = raise_tpl(efi::TPL_HIGH_LEVEL);
  SYSTEM_TIME.fetch_add(time, Ordering::SeqCst);
  let current_time = SYSTEM_TIME.load(Ordering::SeqCst);
  EVENT_DB.timer_tick(current_time);
  restore_tpl(old_tpl); //implicitly dispatches timer notifies if any.
}

fn set_interrupt_state(enable: bool) {
  let cpu_arch_ptr = CPU_ARCH_PTR.load(Ordering::SeqCst);
  if let Some(cpu_arch) = unsafe { cpu_arch_ptr.as_mut() } {
    match enable {
      true => {
        (cpu_arch.enable_interrupt)(cpu_arch_ptr);
      }
      false => {
        (cpu_arch.disable_interrupt)(cpu_arch_ptr);
      }
    };
  }
}

extern "efiapi" fn timer_available_callback(event: efi::Event, _context: *mut c_void) {
  match PROTOCOL_DB.locate_protocol(timer::TIMER_ARCH_PROTOCOL_GUID) {
    Ok(timer_arch_ptr) => {
      let timer_arch_ptr = timer_arch_ptr as *mut timer::TimerArchProtocol;
      let timer_arch = unsafe { &*(timer_arch_ptr) };
      (timer_arch.register_handler)(timer_arch_ptr, timer_tick);
      EVENT_DB.close_event(event).unwrap();
    }
    Err(err) => panic!("Unable to locate timer arch: {:?}", err),
  }
}

extern "efiapi" fn cpu_arch_available(event: efi::Event, _context: *mut c_void) {
  match PROTOCOL_DB.locate_protocol(cpu_arch::PROTOCOL) {
    Ok(cpu_arch_ptr) => {
      CPU_ARCH_PTR.store(cpu_arch_ptr as *mut cpu_arch::Protocol, Ordering::SeqCst);
      EVENT_DB.close_event(event).unwrap();
    }
    Err(err) => panic!("Unable to cpu arch: {:?}", err),
  }
}

pub fn init_events_support(bs: &mut efi::BootServices) {
  bs.create_event = create_event;
  bs.create_event_ex = create_event_ex;
  bs.close_event = close_event;
  bs.signal_event = signal_event;
  bs.wait_for_event = wait_for_event;
  bs.check_event = check_event;
  bs.set_timer = set_timer;
  bs.raise_tpl = raise_tpl;
  bs.restore_tpl = restore_tpl;

  //set up call back for cpu arch protocol installation.
  let event = EVENT_DB
    .create_event(efi::EVT_NOTIFY_SIGNAL, efi::TPL_CALLBACK, Some(cpu_arch_available), None, None)
    .expect("Failed to create timer available callback.");

  PROTOCOL_DB
    .register_protocol_notify(cpu_arch::PROTOCOL, event)
    .expect("Failed to register protocol notify on timer arch callback.");

  //set up call back for timer arch protocol installation.
  let event = EVENT_DB
    .create_event(efi::EVT_NOTIFY_SIGNAL, efi::TPL_CALLBACK, Some(timer_available_callback), None, None)
    .expect("Failed to create timer available callback.");

  PROTOCOL_DB
    .register_protocol_notify(timer::TIMER_ARCH_PROTOCOL_GUID, event)
    .expect("Failed to register protocol notify on timer arch callback.");
}
