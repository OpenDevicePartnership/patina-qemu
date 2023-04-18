use core::{
    convert::TryFrom,
    ffi::c_void,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

use r_efi::{
    eficall, eficall_abi,
    system::{BootServices, EVT_NOTIFY_SIGNAL, TPL_APPLICATION, TPL_CALLBACK, TPL_HIGH_LEVEL},
};

use r_pi::timer::{TIMER_ARCH_PROTOCOL_GUID, TimerArchProtocol};
use uefi_event_lib::{SpinLockedEventDb, TimerDelay};

use crate::protocols::PROTOCOL_DB;

pub static EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

static CURRENT_TPL: AtomicUsize = AtomicUsize::new(TPL_APPLICATION);
static SYSTEM_TIME: AtomicU64 = AtomicU64::new(0);

eficall! {pub fn create_event (
  event_type: u32,
  notify_tpl: r_efi::efi::Tpl,
  notify_function: Option<r_efi::system::EventNotify>,
  notify_context: *mut c_void,
  event: *mut r_efi::efi::Event) -> r_efi::efi::Status {

  if event == core::ptr::null_mut() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  let notify_context = if notify_context != core::ptr::null_mut() {
    Some(notify_context)
  } else {
    None
  };

  let (event_type, event_group) = match event_type {
    r_efi::efi::EVT_SIGNAL_EXIT_BOOT_SERVICES => (
        r_efi::efi::EVT_NOTIFY_SIGNAL,
        Some(r_efi::efi::EVENT_GROUP_EXIT_BOOT_SERVICES)
      ),
    r_efi::efi::EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE => (
        r_efi::efi::EVT_NOTIFY_SIGNAL,
        Some(r_efi::efi::EVENT_GROUP_VIRTUAL_ADDRESS_CHANGE)
      ),
    other => (other, None)
  };

  match EVENT_DB.create_event(event_type, notify_tpl, notify_function, notify_context, event_group) {
    Ok(new_event) => {
      unsafe {*event = new_event};
      r_efi::efi::Status::SUCCESS
    }
    Err(err) => err
  }
}}

eficall! {pub fn create_event_ex (
  event_type: u32,
  notify_tpl: r_efi::efi::Tpl,
  notify_function: Option<r_efi::system::EventNotify>,
  notify_context: *const c_void,
  event_group: *const r_efi::efi::Guid,
  event: *mut r_efi::efi::Event) -> r_efi::efi::Status {

  if event == core::ptr::null_mut() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  let notify_context = if notify_context != core::ptr::null_mut() {
    Some(notify_context as *mut c_void)
  } else {
    None
  };

  match event_type {
    r_efi::efi::EVT_SIGNAL_EXIT_BOOT_SERVICES |
    r_efi::efi::EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE => return r_efi::efi::Status::INVALID_PARAMETER,
    _ => ()
  }

  let event_group = if event_group != core::ptr::null_mut() {
    Some(unsafe {*event_group})
  } else {
    None
  };

  match EVENT_DB.create_event(event_type, notify_tpl, notify_function, notify_context, event_group) {
    Ok(new_event) => {
      unsafe {*event = new_event};
      r_efi::efi::Status::SUCCESS
    }
    Err(err) => err
  }
}}

eficall! {pub fn close_event (event:r_efi::efi::Event) -> r_efi::efi::Status {
  match EVENT_DB.close_event(event) {
    Ok(()) => r_efi::efi::Status::SUCCESS,
    Err(err) => err
  }
}}

eficall! {pub fn signal_event (event: r_efi::efi::Event) -> r_efi::efi::Status {

  let status = match EVENT_DB.signal_event(event) {
    Ok(()) => r_efi::efi::Status::SUCCESS,
    Err(err) => err
  };

  //Note: The C-reference implementation of SignalEvent gets an immediate dispatch of
  //pending events as a side effect of the locking implementation calling raise/restore
  //TPL. The spec doesn't require this; but it's likely that code out there depends
  //on it. So emulate that here with an artificial raise/restore.
  let old_tpl = raise_tpl(TPL_HIGH_LEVEL);
  restore_tpl(old_tpl);

  status
}}

eficall! {pub fn wait_for_event (
  number_of_events: usize,
  event_array: *mut r_efi::efi::Event,
  out_index: *mut usize) -> r_efi::efi::Status {

  if number_of_events == 0 || event_array == core::ptr::null_mut() || out_index == core::ptr::null_mut() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  if CURRENT_TPL.load(Ordering::SeqCst) != TPL_APPLICATION {
    return r_efi::efi::Status::UNSUPPORTED;
  }

  //get the events list as a slice
  let event_list = unsafe {core::slice::from_raw_parts(event_array, number_of_events)};

  //spin on the list
  loop {
    for (index, event) in event_list.iter().enumerate() {
      match check_event(*event) {
        r_efi::efi::Status::NOT_READY => (),
        status => {
          unsafe {*out_index = index};
          return status;
        }
      }
    }
  }

}}

eficall! {pub fn check_event (event:r_efi::efi::Event) -> r_efi::efi::Status {

  let event_type = match EVENT_DB.get_event_type(event) {
    Ok(event_type) => event_type,
    Err(err) => return err
  };

  if event_type.is_notify_signal() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  match EVENT_DB.read_and_clear_signalled(event) {
    Ok(signalled) => if signalled {
      return r_efi::efi::Status::SUCCESS;
    },
    Err(err) => return err
  }

  match EVENT_DB.queue_event_notify(event) {
    Ok(()) => (),
    Err(err) => return err
  }

  // raise/restore TPL to allow notifies to occur at the appropriate level.
  let old_tpl = raise_tpl(TPL_HIGH_LEVEL);
  restore_tpl(old_tpl);

  match EVENT_DB.read_and_clear_signalled(event) {
    Ok(signalled) => if signalled {
      return r_efi::efi::Status::SUCCESS;
    },
    Err(err) => return err
  }

  r_efi::efi::Status::NOT_READY
}}

eficall! {pub fn set_timer (
  event: r_efi::efi::Event,
  timer_type: r_efi::system::TimerDelay,
  trigger_time: u64) -> r_efi::efi::Status {

  let timer_type = match TimerDelay::try_from(timer_type) {
    Err(err) => return err,
    Ok(timer_type) => timer_type
  };

  let (trigger_time, period) = match timer_type {
    TimerDelay::TimerCancel => {
      (None, None)
    },
    TimerDelay::TimerRelative => {
      (Some(SYSTEM_TIME.load(Ordering::SeqCst) + trigger_time), None)
    },
    TimerDelay::TimerPeriodic => {
      (Some(SYSTEM_TIME.load(Ordering::SeqCst) + trigger_time), Some(trigger_time))
    }
  };

  match EVENT_DB.set_timer(event, timer_type, trigger_time, period) {
    Ok(()) => r_efi::efi::Status::SUCCESS,
    Err(err) => err
  }
}}

eficall! { fn raise_tpl (new_tpl: r_efi::efi::Tpl)->r_efi::efi::Tpl {
  let prev_tpl = CURRENT_TPL.fetch_max(new_tpl, Ordering::SeqCst);
  if new_tpl < prev_tpl {
    panic!("Invalid attempt to raise TPL to lower value. New TPL: {:#x?}, Prev TPL: {:#x?}", new_tpl, prev_tpl);
  }
  //todo!("deal with interrupts")
  prev_tpl
}}

eficall! {pub fn restore_tpl (new_tpl: r_efi::efi::Tpl)  {
  let prev_tpl = CURRENT_TPL.fetch_min(new_tpl, Ordering::SeqCst);
  if new_tpl > prev_tpl {
    panic!("Invalid attempt to lower TPL to lower value. New TPL: {:#x?}, Prev TPL: {:#x?}", new_tpl, prev_tpl);
  }

  //todo!("deal with interrupts")

  //dispatch all events higher than current TPL in TPL order.
  for event in EVENT_DB.event_notification_iter(new_tpl) {
    CURRENT_TPL.store(event.notify_tpl, Ordering::SeqCst);
    let notify_context = match event.notify_context {
      Some(context) => context,
      None => core::ptr::null_mut()
    };

    //Caution: this is calling function pointer supplied by code outside DxeRust.
    //The notify_function is not "unsafe" per the signature, even though it's
    //supplied by code outside the dxe_rust module. If it were marked 'unsafe'
    //then other Rust modules executing under DxeRust would need to mark all event
    //callbacks as "unsafe", and the r_efi definition for EventNotify would need to
    //change.
    (event.notify_function)(event.event, notify_context);
  }

  CURRENT_TPL.store(new_tpl, Ordering::SeqCst);

}}

eficall! {fn timer_tick (time: u64) {
  let old_tpl = raise_tpl(TPL_HIGH_LEVEL);
  SYSTEM_TIME.fetch_add(time, Ordering::SeqCst);
  let current_time = SYSTEM_TIME.load(Ordering::SeqCst);
  EVENT_DB.timer_tick(current_time);
  restore_tpl(old_tpl); //implicitly dispatches timer notifies if any.
}}

eficall! {fn timer_available_callback (event: r_efi::efi::Event, _context: *mut c_void) {
  match PROTOCOL_DB.locate_protocol(TIMER_ARCH_PROTOCOL_GUID) {
    Ok(timer_arch_ptr) => {
      let timer_arch_ptr = timer_arch_ptr as *mut TimerArchProtocol;
      let timer_arch = unsafe {&*(timer_arch_ptr)};
      (timer_arch.register_handler)(timer_arch_ptr, timer_tick);
      EVENT_DB.close_event(event).unwrap();
    },
    Err(err) => panic!("Unable to locate timer arch: {:?}", err)
  }
}}

pub fn init_events_support(bs: &mut BootServices) {
    bs.create_event = create_event;
    bs.create_event_ex = create_event_ex;
    bs.close_event = close_event;
    bs.signal_event = signal_event;
    bs.wait_for_event = wait_for_event;
    bs.check_event = check_event;
    bs.set_timer = set_timer;
    bs.raise_tpl = raise_tpl;
    bs.restore_tpl = restore_tpl;

    //set up call back for timer arch protocol installation.
    let event = EVENT_DB
        .create_event(EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(timer_available_callback), None, None)
        .expect("Failed to create timer available callback.");

    PROTOCOL_DB
        .register_protocol_notify(TIMER_ARCH_PROTOCOL_GUID, event)
        .expect("Failed to register protocol notify on timer arch callback.");
}
