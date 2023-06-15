//! # UEFI Event Lib
//! Provides implementation of UEFI event services.
#![no_std]
#![warn(missing_docs)]
#![cfg_attr(feature = "nightly", feature(no_coverage))]

extern crate alloc;

use alloc::{
  collections::{BTreeMap, BTreeSet},
  vec::Vec,
};
use core::{cmp::Ordering, ffi::c_void, fmt};
use r_efi::system::{
  EVT_NOTIFY_SIGNAL, EVT_NOTIFY_WAIT, EVT_SIGNAL_EXIT_BOOT_SERVICES, EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE, EVT_TIMER,
  TPL_APPLICATION, TPL_HIGH_LEVEL,
};

/// Defines the supported UEFI event types
#[repr(u32)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum EventType {
  ///
  /// 0x80000200       Timer event with a notification function that is
  /// queue when the event is signaled with SignalEvent()
  ///
  TimerNotifyEvent = EVT_TIMER | EVT_NOTIFY_SIGNAL,
  ///
  /// 0x80000000       Timer event without a notification function. It can be
  /// signaled with SignalEvent() and checked with CheckEvent() or WaitForEvent().
  ///
  TimerEvent = EVT_TIMER,
  ///
  /// 0x00000100       Generic event with a notification function that
  /// can be waited on with CheckEvent() or WaitForEvent()
  ///
  NotifyWaitEvent = EVT_NOTIFY_WAIT,
  ///
  /// 0x00000200       Generic event with a notification function that
  /// is queue when the event is signaled with SignalEvent()
  ///
  NotifySignalEvent = EVT_NOTIFY_SIGNAL,
  ///
  /// 0x00000201       ExitBootServicesEvent.
  ///
  ExitBootServicesEvent = EVT_SIGNAL_EXIT_BOOT_SERVICES,
  ///
  /// 0x60000202       SetVirtualAddressMapEvent.
  ///
  SetVirtualAddressEvent = EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE,
  ///
  /// 0x00000000       Generic event without a notification function.
  /// It can be signaled with SignalEvent() and checked with CheckEvent()
  /// or WaitForEvent().
  ///
  GenericEvent = 0x00000000,
  ///
  /// 0x80000100       Timer event with a notification function that can be
  /// waited on with CheckEvent() or WaitForEvent()
  ///
  TimerNotifyWaitEvent = EVT_TIMER | EVT_NOTIFY_WAIT,
}

impl TryFrom<u32> for EventType {
  type Error = r_efi::efi::Status;
  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      x if x == EventType::TimerNotifyEvent as u32 => Ok(EventType::TimerNotifyEvent),
      x if x == EventType::TimerEvent as u32 => Ok(EventType::TimerEvent),
      x if x == EventType::NotifyWaitEvent as u32 => Ok(EventType::NotifyWaitEvent),
      x if x == EventType::NotifySignalEvent as u32 => Ok(EventType::NotifySignalEvent),
      //NOTE: the following are placeholders for corresponding event groups; we don't allow them here
      //as the code using the library should do the appropriate translation to event groups before calling create_event
      x if x == EventType::ExitBootServicesEvent as u32 => Err(r_efi::efi::Status::INVALID_PARAMETER),
      x if x == EventType::SetVirtualAddressEvent as u32 => Err(r_efi::efi::Status::INVALID_PARAMETER),
      x if x == EventType::GenericEvent as u32 => Ok(EventType::GenericEvent),
      x if x == EventType::TimerNotifyWaitEvent as u32 => Ok(EventType::TimerNotifyWaitEvent),
      _ => Err(r_efi::efi::Status::INVALID_PARAMETER),
    }
  }
}

impl EventType {
  /// indicates whether this EventType is NOTIFY_SIGNAL
  pub fn is_notify_signal(&self) -> bool {
    (*self as u32) & EVT_NOTIFY_SIGNAL != 0
  }

  /// indicates whether this EventType is NOTIFY_WAIT
  pub fn is_notify_wait(&self) -> bool {
    (*self as u32) & EVT_NOTIFY_WAIT != 0
  }

  /// indicates whether this EventType is TIMER
  pub fn is_timer(&self) -> bool {
    (*self as u32) & EVT_TIMER != 0
  }
}

/// Defines supported timer delay types.
#[repr(u32)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TimerDelay {
  /// Cancels a pending timer
  TimerCancel,
  /// Creates a periodic timer
  TimerPeriodic,
  /// Creates a one-shot relative timer
  TimerRelative,
}

impl TryFrom<u32> for TimerDelay {
  type Error = r_efi::efi::Status;
  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      x if x == TimerDelay::TimerCancel as u32 => Ok(TimerDelay::TimerCancel),
      x if x == TimerDelay::TimerPeriodic as u32 => Ok(TimerDelay::TimerPeriodic),
      x if x == TimerDelay::TimerRelative as u32 => Ok(TimerDelay::TimerRelative),
      _ => Err(r_efi::efi::Status::INVALID_PARAMETER),
    }
  }
}

/// Event Notification
#[derive(Clone)]
pub struct EventNotification {
  /// event handle
  pub event: r_efi::efi::Event,
  /// TPL that notification should run at
  pub notify_tpl: r_efi::efi::Tpl,
  /// notification function
  pub notify_function: r_efi::efi::EventNotify,
  /// context passed to the notification function
  pub notify_context: Option<*mut c_void>,
}

impl fmt::Debug for EventNotification {
  #[cfg_attr(feature = "nightly", feature(no_coverage))]
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("EventNotification")
      .field("event", &self.event)
      .field("notify_tpl", &self.notify_tpl)
      .field("notify_function", &(self.notify_function as usize))
      .field("notify_context", &self.notify_context)
      .finish()
  }
}

//This type is necessary because the HeapSort used to order BTreeSet is not stable with respect
//to insertion order. So we have to tag each event notification as it is added so that we can
//use insertion order as part of the element comparison.
#[derive(Debug, Clone)]
struct TaggedEventNotification(EventNotification, u64);

impl PartialOrd for TaggedEventNotification {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for TaggedEventNotification {
  fn cmp(&self, other: &Self) -> Ordering {
    if self.0.event == other.0.event {
      Ordering::Equal
    } else if self.0.notify_tpl == other.0.notify_tpl {
      self.1.cmp(&other.1)
    } else {
      other.0.notify_tpl.cmp(&self.0.notify_tpl)
    }
  }
}

impl PartialEq for TaggedEventNotification {
  fn eq(&self, other: &Self) -> bool {
    self.0.event == other.0.event
  }
}

impl Eq for TaggedEventNotification {}

// Note: this Event type is a distinct data structure from r_efi::efi::Event.
// Event defined here is a private data structure that tracks the data related to the event,
// whereas r_efi::efi::Event is used as the public index or handle into the event database.
// In the code below r_efi::efi::Event is used to qualify the index/handle type, where as `Event` with
// scope qualification refers to this private type.
struct Event {
  event_id: usize,
  event_type: EventType,
  event_group: Option<r_efi::efi::Guid>,

  signalled: bool,

  //Only used for NOTIFY events.
  notify_tpl: r_efi::efi::Tpl,
  notify_function: Option<r_efi::efi::EventNotify>,
  notify_context: Option<*mut c_void>,

  //Only used for TIMER events.
  trigger_time: Option<u64>,
  period: Option<u64>,
}

impl fmt::Debug for Event {
  #[cfg_attr(feature = "nightly", feature(no_coverage))]
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut notify_func = 0;
    if self.notify_function.is_some() {
      notify_func = self.notify_function.unwrap() as usize;
    }

    f.debug_struct("Event")
      .field("event_id", &self.event_id)
      .field("event_type", &self.event_type)
      .field("event_group", &self.event_group)
      .field("signalled", &self.signalled)
      .field("notify_tpl", &self.notify_tpl)
      .field("notify_function", &notify_func)
      .field("notify_context", &self.notify_context)
      .field("trigger_time", &self.trigger_time)
      .field("period", &self.period)
      .finish()
  }
}

impl Event {
  fn new(
    event_id: usize,
    event_type: u32,
    notify_tpl: r_efi::efi::Tpl,
    notify_function: Option<r_efi::efi::EventNotify>,
    notify_context: Option<*mut c_void>,
    event_group: Option<r_efi::efi::Guid>,
  ) -> Result<Self, r_efi::efi::Status> {
    let notifiable = (event_type & (EVT_NOTIFY_SIGNAL | EVT_NOTIFY_WAIT)) != 0;
    let event_type: EventType = event_type.try_into()?;

    if notifiable {
      if notify_function.is_none() {
        return Err(r_efi::efi::Status::INVALID_PARAMETER);
      }

      // Pedantic check; this will probably not work with "real firmware", so
      // loosen up a bit.
      // match notify_tpl {
      //     TPL_APPLICATION | TPL_CALLBACK | TPL_NOTIFY | TPL_HIGH_LEVEL => (),
      //     _ => return Err(r_efi::efi::Status::INVALID_PARAMETER),
      // }
      if notify_tpl < TPL_APPLICATION || notify_tpl > TPL_HIGH_LEVEL {
        return Err(r_efi::efi::Status::INVALID_PARAMETER);
      }
    }

    Ok(Event {
      event_id,
      event_type,
      notify_tpl,
      notify_function,
      notify_context,
      event_group,
      signalled: false,
      trigger_time: None,
      period: None,
    })
  }
}

struct EventDb {
  events: BTreeMap<usize, Event>,
  next_event_id: usize,
  //TODO: using a BTreeSet here as a priority queue is slower [O(log n)] vs. the
  //per-TPL lists used in the reference C implementation [O(1)] for (de)queueing of event notifies.
  //Benchmarking would need to be done to see whether that perf impact plays out to significantly
  //impact real-world usage.
  pending_notifies: BTreeSet<TaggedEventNotification>,
  notify_tags: u64, //used to ensure that each notify gets a unique tag in increasing order
}

impl EventDb {
  const fn new() -> Self {
    EventDb { events: BTreeMap::new(), next_event_id: 1, pending_notifies: BTreeSet::new(), notify_tags: 0 }
  }

  fn create_event(
    &mut self,
    event_type: u32,
    notify_tpl: r_efi::base::Tpl,
    notify_function: Option<r_efi::system::EventNotify>,
    notify_context: Option<*mut c_void>,
    event_group: Option<r_efi::efi::Guid>,
  ) -> Result<r_efi::efi::Event, r_efi::efi::Status> {
    let id = self.next_event_id;
    self.next_event_id += 1;
    let event = Event::new(id, event_type, notify_tpl, notify_function, notify_context, event_group)?;
    self.events.insert(id, event);
    Ok(id as r_efi::efi::Event)
  }

  fn close_event(&mut self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    let id = event as usize;
    self.events.remove(&id).ok_or(r_efi::efi::Status::INVALID_PARAMETER)?;
    Ok(())
  }

  //private helper function for signal_event.
  fn queue_notify_event(pending_notifies: &mut BTreeSet<TaggedEventNotification>, event: &mut Event, tag: u64) {
    if event.event_type.is_notify_signal() || event.event_type.is_notify_wait() {
      pending_notifies.insert(TaggedEventNotification {
        0: EventNotification {
          event: event.event_id as r_efi::efi::Event,
          notify_tpl: event.notify_tpl,
          notify_function: event.notify_function.unwrap(),
          notify_context: event.notify_context,
        },
        1: tag,
      });
    }
  }

  fn signal_event(&mut self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    let id = event as usize;
    let mut current_event = self.events.get_mut(&id).ok_or(r_efi::efi::Status::INVALID_PARAMETER)?;

    //signal all the members of the same event group (including the current one), if present.
    if let Some(target_group) = current_event.event_group {
      self.signal_group(target_group);
    } else {
      // if no group, signal the event by itself.
      current_event.signalled = true;
      if current_event.event_type.is_notify_signal() {
        Self::queue_notify_event(&mut self.pending_notifies, current_event, self.notify_tags);
        self.notify_tags += 1;
      }
    }
    Ok(())
  }

  fn signal_group(&mut self, group: r_efi::efi::Guid) {
    for member_event in self.events.values_mut().filter(|e| e.event_group == Some(group)) {
      member_event.signalled = true;
      if member_event.event_type.is_notify_signal() {
        Self::queue_notify_event(&mut self.pending_notifies, member_event, self.notify_tags);
        self.notify_tags += 1;
      }
    }
  }

  fn clear_signal(&mut self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    let id = event as usize;
    let mut event = self.events.get_mut(&id).ok_or(r_efi::efi::Status::INVALID_PARAMETER)?;
    event.signalled = false;
    Ok(())
  }

  fn is_signalled(&mut self, event: r_efi::efi::Event) -> bool {
    let id = event as usize;
    if let Some(event) = self.events.get(&id) {
      event.signalled
    } else {
      false
    }
  }

  fn queue_event_notify(&mut self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    let id = event as usize;
    let current_event = self.events.get_mut(&id).ok_or(r_efi::efi::Status::INVALID_PARAMETER)?;

    Self::queue_notify_event(&mut self.pending_notifies, current_event, self.notify_tags);
    self.notify_tags += 1;

    Ok(())
  }

  fn get_event_type(&mut self, event: r_efi::efi::Event) -> Result<EventType, r_efi::efi::Status> {
    let id = event as usize;
    Ok(self.events.get(&id).ok_or(r_efi::efi::Status::INVALID_PARAMETER)?.event_type)
  }

  fn get_notification_data(&mut self, event: r_efi::efi::Event) -> Result<EventNotification, r_efi::efi::Status> {
    let id = event as usize;
    if let Some(found_event) = self.events.get(&id) {
      if (found_event.event_type as u32) & (EVT_NOTIFY_SIGNAL | EVT_NOTIFY_WAIT) == 0 {
        return Err(r_efi::efi::Status::NOT_FOUND);
      }
      Ok(EventNotification {
        event: event,
        notify_tpl: found_event.notify_tpl,
        notify_function: found_event.notify_function.expect("Notify event without notify function is illegal."),
        notify_context: found_event.notify_context,
      })
    } else {
      Err(r_efi::efi::Status::NOT_FOUND)
    }
  }

  fn set_timer(
    &mut self,
    event: r_efi::efi::Event,
    timer_type: TimerDelay,
    trigger_time: Option<u64>,
    period: Option<u64>,
  ) -> Result<(), r_efi::efi::Status> {
    let id = event as usize;
    if let Some(event) = self.events.get_mut(&id) {
      if !event.event_type.is_timer() {
        return Err(r_efi::efi::Status::INVALID_PARAMETER);
      }
      match timer_type {
        TimerDelay::TimerCancel => {
          if trigger_time.is_some() || period.is_some() {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
          }
        }
        TimerDelay::TimerPeriodic => {
          if trigger_time.is_none() || period.is_none() {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
          }
        }
        TimerDelay::TimerRelative => {
          if trigger_time.is_none() || period.is_some() {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
          }
        }
      }
      event.trigger_time = trigger_time;
      event.period = period;
      Ok(())
    } else {
      Err(r_efi::efi::Status::INVALID_PARAMETER)
    }
  }

  fn timer_tick(&mut self, current_time: u64) {
    let events: Vec<usize> = self.events.keys().cloned().collect();
    for event in events {
      let current_event = self.events.get_mut(&event).unwrap();
      if current_event.event_type.is_timer() {
        if let Some(trigger_time) = current_event.trigger_time {
          if trigger_time <= current_time {
            if let Some(period) = current_event.period {
              current_event.trigger_time = Some(current_time + period);
            } else {
              //no period means it's a one-shot event; another call to set_timer is required to "re-arm"
              current_event.trigger_time = None;
            }
            self.signal_event(event as *mut c_void).unwrap();
          }
        }
      }
    }
  }

  fn consume_next_event_notify(&mut self, tpl_level: r_efi::efi::Tpl) -> Option<EventNotification> {
    //if items at front of queue don't exist (e.g. due to close_event), silently pop them off.
    while let Some(item) = self.pending_notifies.first() {
      if !self.events.contains_key(&(item.0.event as usize)) {
        self.pending_notifies.pop_first();
      } else {
        break;
      }
    }
    //if item at front of queue is not higher than desired TPL, then return none
    //otherwise, pop it off, mark it signalled, and return it.
    if let Some(item) = self.pending_notifies.first() {
      if item.0.notify_tpl <= tpl_level {
        return None;
      } else {
        let item = self.pending_notifies.pop_first().unwrap();
        self.events.get_mut(&(item.0.event as usize)).unwrap().signalled = false;
        return Some(item.0);
      }
    }
    None
  }

  fn is_valid(&mut self, event: r_efi::efi::Event) -> bool {
    self.events.contains_key(&(event as usize))
  }
}

struct EventNotificationIterator {
  event_db: &'static SpinLockedEventDb,
  tpl_level: r_efi::efi::Tpl,
}

impl EventNotificationIterator {
  fn new(event_db: &'static SpinLockedEventDb, tpl_level: r_efi::efi::Tpl) -> Self {
    EventNotificationIterator { event_db, tpl_level }
  }
}

impl Iterator for EventNotificationIterator {
  type Item = EventNotification;
  fn next(&mut self) -> Option<EventNotification> {
    self.event_db.lock().consume_next_event_notify(self.tpl_level)
  }
}

/// # Spin-Locked UEFI Event database
/// Implements UEFI event database support using a spinlock for mutex.
pub struct SpinLockedEventDb {
  inner: spin::Mutex<EventDb>,
}

impl SpinLockedEventDb {
  /// Creates a new instance of EventDb.
  pub const fn new() -> Self {
    SpinLockedEventDb { inner: spin::Mutex::new(EventDb::new()) }
  }

  fn lock(&self) -> spin::MutexGuard<EventDb> {
    self.inner.lock()
  }

  /// creates a new event in the event database
  pub fn create_event(
    &self,
    event_type: u32,
    notify_tpl: r_efi::base::Tpl,
    notify_function: Option<r_efi::system::EventNotify>,
    notify_context: Option<*mut c_void>,
    event_group: Option<r_efi::efi::Guid>,
  ) -> Result<r_efi::efi::Event, r_efi::efi::Status> {
    self.lock().create_event(event_type, notify_tpl, notify_function, notify_context, event_group)
  }

  /// closes (deletes) an event from the event database
  pub fn close_event(&self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    self.lock().close_event(event)
  }

  /// marks an event as signalled, and queues it for dispatch if it is of type NotifySignalEvent
  pub fn signal_event(&self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    self.lock().signal_event(event)
  }

  /// signals an event group
  pub fn signal_group(&self, group: r_efi::efi::Guid) {
    self.lock().signal_group(group)
  }

  /// returns the event type for the given event
  pub fn get_event_type(&self, event: r_efi::efi::Event) -> Result<EventType, r_efi::efi::Status> {
    self.lock().get_event_type(event)
  }

  /// indicates whether the given event is in the signalled state
  pub fn is_signalled(&self, event: r_efi::efi::Event) -> bool {
    self.lock().is_signalled(event)
  }

  /// clears the signalled state
  pub fn clear_signal(&self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    self.lock().clear_signal(event)
  }

  /// reads and clears the signalled state
  pub fn read_and_clear_signalled(&self, event: r_efi::efi::Event) -> Result<bool, r_efi::efi::Status> {
    let mut event_db = self.lock();
    let signalled = event_db.is_signalled(event);
    if signalled {
      event_db.clear_signal(event)?;
    }
    Ok(signalled)
  }

  /// queues the notification function associated with the given event (if any) for dispatch
  pub fn queue_event_notify(&self, event: r_efi::efi::Event) -> Result<(), r_efi::efi::Status> {
    self.lock().queue_event_notify(event)
  }

  /// returns the notification data associated with the event.
  pub fn get_notification_data(&self, event: r_efi::efi::Event) -> Result<EventNotification, r_efi::efi::Status> {
    self.lock().get_notification_data(event)
  }

  /// sets a timer on the specified event
  pub fn set_timer(
    &self,
    event: r_efi::efi::Event,
    timer_type: TimerDelay,
    trigger_time: Option<u64>,
    period: Option<u64>,
  ) -> Result<(), r_efi::efi::Status> {
    self.lock().set_timer(event, timer_type, trigger_time, period)
  }

  /// called to advance the system time and process any timer events that fire
  pub fn timer_tick(&self, current_time: u64) {
    self.lock().timer_tick(current_time);
  }

  /// returns an iterator over pending event notifications that should be dispatched
  /// at or above the given TPL level. Note: any new events added to the dispatch
  /// queue between calls to next() on the iterator will also be returned by the
  /// iterator - the iterator will only stop if there are no pending dispatches
  /// at or above the given TPL on a call to next().
  pub fn event_notification_iter(&'static self, tpl_level: r_efi::efi::Tpl) -> impl Iterator<Item = EventNotification> {
    EventNotificationIterator::new(self, tpl_level)
  }

  /// Indicates whether a given event is valid
  pub fn is_valid(&self, event: r_efi::efi::Event) -> bool {
    self.lock().is_valid(event)
  }
}

unsafe impl Send for SpinLockedEventDb {}
unsafe impl Sync for SpinLockedEventDb {}

#[cfg(test)]
mod tests {
  extern crate std;
  use core::str::FromStr;

  use alloc::{vec, vec::Vec};
  use r_efi::{
    eficall, eficall_abi,
    system::{EVT_NOTIFY_SIGNAL, EVT_TIMER, TPL_CALLBACK, TPL_HIGH_LEVEL, TPL_NOTIFY},
  };
  use uuid::Uuid;

  use super::*;

  #[test]
  fn new_should_create_event_db_local() {
    //Note: for coverage, here we create the SpinLockedEventDb on the stack. But all the other tests create it as
    //'static' to mimic expected usage.
    let spin_locked_event_db: SpinLockedEventDb = SpinLockedEventDb::new();
    let events = &spin_locked_event_db.lock().events;
    assert_eq!(events.len(), 0);
  }

  #[test]
  fn new_should_create_event_db() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    assert_eq!(SPIN_LOCKED_EVENT_DB.lock().events.len(), 0)
  }

  eficall! {fn test_notify_function(_:r_efi::efi::Event, _:*mut core::ffi::c_void){}}

  #[test]
  fn create_event_should_create_event() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let result = SPIN_LOCKED_EVENT_DB.create_event(
      EVT_TIMER | EVT_NOTIFY_SIGNAL,
      TPL_NOTIFY,
      Some(test_notify_function),
      None,
      None,
    );
    assert!(result.is_ok());
    let event = result.unwrap();
    let index = event as usize;
    assert!(&index < &SPIN_LOCKED_EVENT_DB.lock().next_event_id);
    let events = &SPIN_LOCKED_EVENT_DB.lock().events;
    assert_eq!(events.get(&index).unwrap().event_type, EventType::TimerNotifyEvent);
    assert_eq!(events.get(&index).unwrap().event_type as u32, EVT_TIMER | EVT_NOTIFY_SIGNAL);
    assert_eq!(events.get(&index).unwrap().notify_tpl, TPL_NOTIFY);
    assert_eq!(events.get(&index).unwrap().notify_function.unwrap() as usize, test_notify_function as usize);
    assert_eq!(events.get(&index).unwrap().notify_context, None);
    assert_eq!(events.get(&index).unwrap().event_group, None);
  }

  #[test]
  fn create_event_with_bad_input_should_not_create_event() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    //Try with an invalid event type.
    let result = SPIN_LOCKED_EVENT_DB.create_event(EVT_SIGNAL_EXIT_BOOT_SERVICES, TPL_NOTIFY, None, None, None);
    assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));

    //if type has EVT_NOTIFY_SIGNAL or EVT_NOTIFY_WAIT, then NotifyFunction must be non-NULL and NotifyTpl must be a valid TPL.
    //Try to create a notified event with None notify_function - should fail.
    let result = SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, None, None, None);
    assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));

    //Try to create a notified event with Some notify_function but invalid TPL - should fail.
    let result = SPIN_LOCKED_EVENT_DB.create_event(
      EVT_TIMER | EVT_NOTIFY_SIGNAL,
      TPL_HIGH_LEVEL + 1,
      Some(test_notify_function),
      None,
      None,
    );
    assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));
  }

  #[test]
  fn close_event_should_delete_event() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let mut events: Vec<r_efi::efi::Event> = Vec::new();
    for _ in 0..10 {
      events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
          .unwrap(),
      );
    }
    for consumed in 1..11 {
      let event = events.pop().unwrap();
      assert!(SPIN_LOCKED_EVENT_DB.is_valid(event));
      let result = SPIN_LOCKED_EVENT_DB.close_event(event);
      assert!(result.is_ok());
      assert_eq!(SPIN_LOCKED_EVENT_DB.lock().events.len(), 10 - consumed);
      assert!(!SPIN_LOCKED_EVENT_DB.is_valid(event));
    }
  }

  #[test]
  fn signal_event_should_put_events_in_signalled_state() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let mut events: Vec<r_efi::efi::Event> = Vec::new();
    for _ in 0..10 {
      events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
          .unwrap(),
      );
    }

    for event in events {
      let result = SPIN_LOCKED_EVENT_DB.signal_event(event);
      assert!(result.is_ok());
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }
  }

  #[test]
  fn signal_event_on_an_event_group_should_put_all_members_in_signalled_state() {
    let uuid = Uuid::from_str("aefcf33c-ce02-47b4-89f6-4bacdeda3377").unwrap();
    let group1: r_efi::efi::Guid = unsafe { core::mem::transmute(*uuid.as_bytes()) };
    let uuid = Uuid::from_str("3a08a8c7-054b-4268-8aed-bc6a3aef999f").unwrap();
    let group2: r_efi::efi::Guid = unsafe { core::mem::transmute(*uuid.as_bytes()) };
    let uuid = Uuid::from_str("745e8316-4889-4f58-be3c-6b718b7170ec").unwrap();
    let group3: r_efi::efi::Guid = unsafe { core::mem::transmute(*uuid.as_bytes()) };

    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let mut group1_events: Vec<r_efi::efi::Event> = Vec::new();
    let mut group2_events: Vec<r_efi::efi::Event> = Vec::new();
    let mut group3_events: Vec<r_efi::efi::Event> = Vec::new();
    let mut ungrouped_events: Vec<r_efi::efi::Event> = Vec::new();

    for _ in 0..10 {
      group1_events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, Some(group1))
          .unwrap(),
      );
    }

    for _ in 0..10 {
      group2_events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, Some(group2))
          .unwrap(),
      );
    }

    for _ in 0..10 {
      group3_events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, Some(group3))
          .unwrap(),
      );
    }

    for _ in 0..10 {
      ungrouped_events.push(
        SPIN_LOCKED_EVENT_DB
          .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
          .unwrap(),
      );
    }

    //signal an ungrouped event
    SPIN_LOCKED_EVENT_DB.signal_event(ungrouped_events.pop().unwrap()).unwrap();

    //all other events should remain unsignalled
    for event in group1_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    for event in group2_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    for event in ungrouped_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //signal an event in a group
    SPIN_LOCKED_EVENT_DB.signal_event(group1_events[0]).unwrap();

    //events in the same group should be signalled.
    for event in group1_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //events in another group should not be signalled.
    for event in group2_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //ungrouped events should not be signalled.
    for event in ungrouped_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //signal an event in a different group
    SPIN_LOCKED_EVENT_DB.signal_event(group2_events[0]).unwrap();

    //first event group should remain signalled.
    for event in group1_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //second event group should now be signalled.
    for event in group2_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //third event group should not be signalled.
    for event in group3_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //signal events in third group using signal_group
    SPIN_LOCKED_EVENT_DB.signal_group(group3);
    //first event group should remain signalled.
    for event in group1_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //second event group should remain signalled.
    for event in group2_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //third event group should now be signalled.
    for event in group3_events.clone() {
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }

    //ungrouped events should not be signalled.
    for event in ungrouped_events.clone() {
      assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    }
  }

  #[test]
  fn clear_signal_should_clear_signalled_state() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(event).unwrap();
    assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    let result = SPIN_LOCKED_EVENT_DB.clear_signal(event);
    assert!(result.is_ok());
    assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
  }

  #[test]
  fn is_signalled_should_return_false_for_closed_or_non_existent_event() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(event).unwrap();
    assert!(SPIN_LOCKED_EVENT_DB.is_signalled(event));
    SPIN_LOCKED_EVENT_DB.close_event(event).unwrap();
    assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(event));
    assert!(!SPIN_LOCKED_EVENT_DB.is_signalled(0x1234 as *mut c_void));
  }

  #[test]
  fn signalled_events_with_notifies_should_be_put_in_pending_queue_in_tpl_order() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();
    let callback_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();
    let notify_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    let notify_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    let high_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_HIGH_LEVEL, Some(test_notify_function), None, None)
      .unwrap();
    let high_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_HIGH_LEVEL, Some(test_notify_function), None, None)
      .unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt1).unwrap();

    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt2).unwrap();

    {
      let mut event_db = SPIN_LOCKED_EVENT_DB.lock();
      let queue = &mut event_db.pending_notifies;
      assert_eq!(queue.pop_first().unwrap().0.event, high_evt1);
      assert_eq!(queue.pop_first().unwrap().0.event, high_evt2);
      assert_eq!(queue.pop_first().unwrap().0.event, notify_evt1);
      assert_eq!(queue.pop_first().unwrap().0.event, notify_evt2);
      assert_eq!(queue.pop_first().unwrap().0.event, callback_evt1);
      assert_eq!(queue.pop_first().unwrap().0.event, callback_evt2);
    }
  }

  #[test]
  fn signalled_event_iterator_should_return_next_events_in_tpl_order() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      0
    );

    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();
    let callback_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();
    let notify_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    let notify_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();
    let high_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_HIGH_LEVEL, Some(test_notify_function), None, None)
      .unwrap();
    let high_evt2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_HIGH_LEVEL, Some(test_notify_function), None, None)
      .unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt1).unwrap();

    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt2).unwrap();

    for (event_notification, expected_event) in
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_NOTIFY).zip(vec![high_evt1, high_evt2])
    {
      assert_eq!(event_notification.event, expected_event);
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(expected_event) == false);
    }

    //re-signal the consumed events
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt2).unwrap();

    for (event_notification, expected_event) in SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_CALLBACK).zip(vec![
      high_evt1,
      high_evt2,
      notify_evt1,
      notify_evt2,
    ]) {
      assert_eq!(event_notification.event, expected_event);
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(expected_event) == false);
    }

    //re-signal the consumed events
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt2).unwrap();

    for (event_notification, expected_event) in SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).zip(vec![
      high_evt1,
      high_evt2,
      notify_evt1,
      notify_evt2,
      callback_evt1,
      callback_evt2,
    ]) {
      assert_eq!(event_notification.event, expected_event);
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(expected_event) == false);
    }

    //re-signal the consumed events
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(high_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(notify_evt2).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt2).unwrap();

    //close or clear some of the events before consuming
    SPIN_LOCKED_EVENT_DB.close_event(high_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.close_event(notify_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.close_event(callback_evt1).unwrap();

    for (event_notification, expected_event) in
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).zip(vec![high_evt2, notify_evt2, callback_evt2])
    {
      assert_eq!(event_notification.event, expected_event);
      assert!(SPIN_LOCKED_EVENT_DB.is_signalled(expected_event) == false);
    }
  }

  #[test]
  fn signalling_an_event_more_than_once_should_not_queue_it_more_than_once() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();

    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();

    {
      let db = SPIN_LOCKED_EVENT_DB.lock();
      assert_eq!(db.pending_notifies.len(), 1);
    }
    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      1
    );
  }

  #[test]
  fn read_and_clear_signalled_should_clear_signal() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();

    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();

    {
      let db = SPIN_LOCKED_EVENT_DB.lock();
      assert_eq!(db.pending_notifies.len(), 1);
    }

    let result = SPIN_LOCKED_EVENT_DB.read_and_clear_signalled(callback_evt1);
    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result, true);
    let result = SPIN_LOCKED_EVENT_DB.read_and_clear_signalled(callback_evt1);
    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result, false);
  }

  #[test]
  fn signalling_a_notify_wait_event_should_not_queue_it() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    let callback_evt1 =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_NOTIFY_WAIT, TPL_CALLBACK, Some(test_notify_function), None, None).unwrap();

    SPIN_LOCKED_EVENT_DB.signal_event(callback_evt1).unwrap();

    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      0
    );
  }

  #[test]
  fn queue_event_notify_should_queue_event_notify() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();

    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();

    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      1
    );
  }

  #[test]
  fn queue_event_notify_should_work_for_both_notify_wait_and_notify_signal() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();

    let callback_evt1 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(test_notify_function), None, None)
      .unwrap();

    let callback_evt2 =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_NOTIFY_WAIT, TPL_CALLBACK, Some(test_notify_function), None, None).unwrap();

    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt1).unwrap();
    SPIN_LOCKED_EVENT_DB.queue_event_notify(callback_evt2).unwrap();

    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      2
    );
  }

  #[test]
  fn get_event_type_should_return_event_type() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    let result = SPIN_LOCKED_EVENT_DB.get_event_type(event);
    assert_eq!(result.unwrap(), EventType::TimerNotifyEvent);

    let event = (event as usize + 1) as *mut c_void;
    let result = SPIN_LOCKED_EVENT_DB.get_event_type(event);
    assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));
  }

  #[test]
  fn get_notification_data_should_return_notification_data() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let test_context: *mut c_void = 0x1234 as *mut c_void;
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), Some(test_context), None)
      .unwrap();

    let notification_data = SPIN_LOCKED_EVENT_DB.get_notification_data(event);
    assert!(notification_data.is_ok());
    let event_notification = notification_data.unwrap();
    assert_eq!(event_notification.notify_tpl, TPL_NOTIFY);
    assert_eq!(event_notification.notify_function as usize, test_notify_function as usize);
    assert_eq!(event_notification.notify_context.unwrap(), test_context);

    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    let notification_data = SPIN_LOCKED_EVENT_DB.get_notification_data(event);
    assert!(notification_data.is_ok());
    let event_notification = notification_data.unwrap();
    assert_eq!(event_notification.notify_tpl, TPL_NOTIFY);
    assert_eq!(event_notification.notify_function as usize, test_notify_function as usize);
    assert!(event_notification.notify_context.is_none());

    let event = SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER, TPL_NOTIFY, None, None, None).unwrap();
    let notification_data = SPIN_LOCKED_EVENT_DB.get_notification_data(event);
    assert_eq!(notification_data.err(), Some(r_efi::efi::Status::NOT_FOUND));

    let notification_data = SPIN_LOCKED_EVENT_DB.get_notification_data(0x1234 as *mut c_void);
    assert_eq!(notification_data.err(), Some(r_efi::efi::Status::NOT_FOUND));
  }

  #[test]
  fn set_timer_on_event_should_set_timer_on_event() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER, TPL_NOTIFY, Some(test_notify_function), None, None).unwrap();

    let index = event as usize;

    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerRelative, Some(0x100), None);
    assert!(result.is_ok());
    {
      let events = &SPIN_LOCKED_EVENT_DB.lock().events;
      assert_eq!(events.get(&index).unwrap().trigger_time, Some(0x100));
      assert_eq!(events.get(&index).unwrap().period, None);
    }

    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerPeriodic, Some(0x100), Some(0x200));
    assert!(result.is_ok());
    {
      let events = &SPIN_LOCKED_EVENT_DB.lock().events;
      assert_eq!(events.get(&index).unwrap().trigger_time, Some(0x100));
      assert_eq!(events.get(&index).unwrap().period, Some(0x200));
    }

    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerCancel, None, None);
    assert!(result.is_ok());
    {
      let events = &SPIN_LOCKED_EVENT_DB.lock().events;
      assert_eq!(events.get(&index).unwrap().trigger_time, None);
      assert_eq!(events.get(&index).unwrap().period, None);
    }

    let event =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None).unwrap();

    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerPeriodic, Some(0x100), Some(0x200));
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));

    let event =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER, TPL_NOTIFY, Some(test_notify_function), None, None).unwrap();
    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerCancel, Some(0x100), None);
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));

    let event =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER, TPL_NOTIFY, Some(test_notify_function), None, None).unwrap();
    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerPeriodic, None, None);
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));

    let event =
      SPIN_LOCKED_EVENT_DB.create_event(EVT_TIMER, TPL_NOTIFY, Some(test_notify_function), None, None).unwrap();
    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerRelative, None, Some(0x100));
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));

    let result = SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerRelative, None, Some(0x100));
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));

    let result = SPIN_LOCKED_EVENT_DB.set_timer(0x1234 as *mut c_void, TimerDelay::TimerRelative, Some(0x100), None);
    assert_eq!(result.err(), Some(r_efi::efi::Status::INVALID_PARAMETER));
  }

  #[test]
  fn timer_tick_should_signal_expired_timers() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    let event2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerRelative, Some(0x100), None).unwrap();
    SPIN_LOCKED_EVENT_DB.set_timer(event2, TimerDelay::TimerRelative, Some(0x400), None).unwrap();
    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      0
    );

    //tick past the first timer
    SPIN_LOCKED_EVENT_DB.timer_tick(0x200);

    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, event);

    //tick again, but not enough to trigger second timer.
    SPIN_LOCKED_EVENT_DB.timer_tick(0x300);

    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 0);

    //tick past the second timer.
    SPIN_LOCKED_EVENT_DB.timer_tick(0x400);

    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, event2);
  }

  #[test]
  fn periodic_timers_should_rearm_after_tick() {
    static SPIN_LOCKED_EVENT_DB: SpinLockedEventDb = SpinLockedEventDb::new();
    let event = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    let event2 = SPIN_LOCKED_EVENT_DB
      .create_event(EVT_TIMER | EVT_NOTIFY_SIGNAL, TPL_NOTIFY, Some(test_notify_function), None, None)
      .unwrap();

    SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerPeriodic, Some(0x100), Some(0x100)).unwrap();
    SPIN_LOCKED_EVENT_DB.set_timer(event2, TimerDelay::TimerPeriodic, Some(0x500), Some(0x500)).unwrap();

    assert_eq!(
      SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>().len(),
      0
    );

    //tick past the first timer
    SPIN_LOCKED_EVENT_DB.timer_tick(0x100);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, event);

    //tick just prior to re-armed first timer
    SPIN_LOCKED_EVENT_DB.timer_tick(0x1FF);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 0);

    //tick past the re-armed first timer
    SPIN_LOCKED_EVENT_DB.timer_tick(0x210);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, event);

    //tick past the second timer.
    SPIN_LOCKED_EVENT_DB.timer_tick(0x500);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, event);
    assert_eq!(events[1].event, event2);

    //tick past the rearmed first timer
    SPIN_LOCKED_EVENT_DB.timer_tick(0x600);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, event);

    //cancel the first timer
    SPIN_LOCKED_EVENT_DB.set_timer(event, TimerDelay::TimerCancel, None, None).unwrap();

    //tick past where it would have been.
    SPIN_LOCKED_EVENT_DB.timer_tick(0x700);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 0);

    //close the event for the second timer
    SPIN_LOCKED_EVENT_DB.close_event(event2).unwrap();

    //tick past where it would have been.
    SPIN_LOCKED_EVENT_DB.timer_tick(0x1000);
    let events = SPIN_LOCKED_EVENT_DB.event_notification_iter(TPL_APPLICATION).collect::<Vec<EventNotification>>();
    assert_eq!(events.len(), 0);
  }
}
