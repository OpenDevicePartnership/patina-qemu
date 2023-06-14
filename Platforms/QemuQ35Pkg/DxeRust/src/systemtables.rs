//Routines for creating and manipulating EFI System tables

use core::{ffi::c_void, mem::size_of, slice::from_raw_parts};

use alloc::{alloc::Allocator, boxed::Box};
use r_efi::{
  efi::{Boolean, Char16, Event, Guid, Handle, PhysicalAddress, Status, Tpl},
  eficall, eficall_abi,
  protocols::{device_path, simple_text_input, simple_text_output},
  system::{BootServices, RuntimeServices, SystemTable, TableHeader},
};

use crate::allocator::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR;

pub struct EfiRuntimeServicesTable {
  runtime_services: Box<RuntimeServices, &'static dyn Allocator>,
}

impl EfiRuntimeServicesTable {
  //private unimplemented stub functions used to initialize the table.
  eficall! {fn get_time_unimplemented (_: *mut r_efi::system::Time, _: *mut r_efi::system::TimeCapabilities) -> Status {
    unimplemented!()
  }}

  eficall! {fn set_time_unimplemented (_: *mut r_efi::system::Time) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_wakeup_time_unimplemented (_: *mut Boolean, _: *mut Boolean, _: *mut r_efi::system::Time)-> Status {
    unimplemented!()
  }}

  eficall! {fn set_wakeup_time_unimplemented (_: Boolean, _: *mut r_efi::system::Time) -> Status {
    unimplemented!()
  }}

  eficall! {fn set_virtual_address_map_unimplemented (_: usize, _: usize, _: u32, _: *mut r_efi::system::MemoryDescriptor) -> Status {
    unimplemented!()
  }}

  eficall! {fn convert_pointer_unimplemented (_: usize, _: *mut *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_variable_unimplemented (_: *mut Char16, _: *mut Guid, _: *mut u32, _: *mut usize, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_next_variable_name_unimplemented (_: *mut usize, _: *mut Char16, _: *mut Guid) -> Status {
    unimplemented!()
  }}

  eficall! {fn set_variable_unimplemented (_: *mut Char16, _: *mut Guid, _: u32, _: usize, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_next_high_mono_count_unimplemented (_: *mut u32) -> Status {
    unimplemented!()
  }}

  eficall! {fn reset_system_unimplemented (_: r_efi::system::ResetType, _: Status, _: usize, _: *mut c_void) {
    unimplemented!()
  }}

  eficall! {fn update_capsule_unimplemented (_: *mut *mut r_efi::system::CapsuleHeader, _: usize, _: PhysicalAddress) -> Status {
    unimplemented!()
  }}

  eficall! {fn query_capsule_capabilities_unimplemented (_: *mut *mut r_efi::system::CapsuleHeader, _: usize, _: *mut u64, _: *mut r_efi::system::ResetType) -> Status {
    unimplemented!()
  }}

  eficall! {fn query_variable_info_unimplemented (_: u32, _: *mut u64, _: *mut u64, _: *mut u64) -> Status {
    unimplemented!()
  }}

  pub fn init_runtime_services_table() -> EfiRuntimeServicesTable {
    let mut rt = RuntimeServices {
      hdr: TableHeader {
        signature: r_efi::system::RUNTIME_SERVICES_SIGNATURE,
        revision: r_efi::system::RUNTIME_SERVICES_REVISION,
        header_size: 0,
        crc32: 0,
        reserved: 0,
      },
      get_time: Self::get_time_unimplemented,
      set_time: Self::set_time_unimplemented,
      get_wakeup_time: Self::get_wakeup_time_unimplemented,
      set_wakeup_time: Self::set_wakeup_time_unimplemented,
      set_virtual_address_map: Self::set_virtual_address_map_unimplemented,
      convert_pointer: Self::convert_pointer_unimplemented,
      get_variable: Self::get_variable_unimplemented,
      get_next_variable_name: Self::get_next_variable_name_unimplemented,
      set_variable: Self::set_variable_unimplemented,
      get_next_high_mono_count: Self::get_next_high_mono_count_unimplemented,
      reset_system: Self::reset_system_unimplemented,
      update_capsule: Self::update_capsule_unimplemented,
      query_capsule_capabilities: Self::query_capsule_capabilities_unimplemented,
      query_variable_info: Self::query_variable_info_unimplemented,
    };

    rt.hdr.header_size = size_of::<RuntimeServices>() as u32;
    let rt_ptr = &rt as *const RuntimeServices as *const u8;
    let rt_slice = unsafe { from_raw_parts(rt_ptr, size_of::<RuntimeServices>()) };
    rt.hdr.crc32 = crc32fast::hash(rt_slice);

    EfiRuntimeServicesTable { runtime_services: Box::new_in(rt, &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR) }
  }
}

pub struct EfiBootServicesTable {
  boot_services: Box<BootServices>, //Use the global allocator (EfiBootServicesData)
}

impl EfiBootServicesTable {
  //private unimplemented stub functions used to initialize the table.
  eficall! { fn raise_tpl_unimplemented (_: Tpl)->Tpl {
    unimplemented!()
  }}

  eficall! {fn restore_tpl_unimplemented (_: Tpl) {
    unimplemented!()
  }}

  eficall! {fn allocate_pages_unimplemented (_: r_efi::system::AllocateType, _: r_efi::system::MemoryType, _: usize, _: *mut PhysicalAddress) -> Status {
    unimplemented!()
  }}

  eficall! {fn free_pages_unimplemented (_:PhysicalAddress, _:usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_memory_map_unimplemented (_: *mut usize, _: *mut r_efi::system::MemoryDescriptor, _: *mut usize, _:*mut usize, _:*mut u32) -> Status {
    unimplemented!()
  }}

  eficall! {fn allocate_pool_unimplemented (_: r_efi::system::MemoryType, _: usize, _: *mut *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn free_pool_unimplemented (_: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn create_event_unimplemented (_: u32, _: Tpl, _: Option<r_efi::system::EventNotify>, _: *mut c_void, _: *mut Event) -> Status {
    unimplemented!()
  }}

  eficall! {fn set_timer_unimplemented (_: Event, _: r_efi::system::TimerDelay, _: u64) -> Status {
    unimplemented!()
  }}

  eficall! {fn wait_for_event_unimplemented (_: usize, _: *mut Event, _: *mut usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn signal_event_unimplemented (_: Event) -> Status {
    unimplemented!()
  }}

  eficall! {fn close_event_unimplemented (_:Event) -> Status {
    unimplemented!()
  }}

  eficall! {fn check_event_unimplemented (_:Event) -> Status {
    unimplemented!()
  }}

  eficall! {fn install_protocol_interface_unimplemented (_: *mut Handle, _: *mut Guid, _: r_efi::system::InterfaceType, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn reinstall_protocol_interface_unimplemented (_: Handle, _: *mut Guid, _: *mut c_void, _: *mut c_void)->Status {
    unimplemented!()
  }}

  eficall! {fn uninstall_protocol_interface_unimplemented (_: Handle, _: *mut Guid, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn handle_protocol_unimplemented (_: Handle, _: *mut Guid, _: *mut *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn register_protocol_notify_unimplemented (_: *mut Guid, _: Event, _: *mut *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn locate_handle_unimplemented (_: r_efi::system::LocateSearchType, _: *mut Guid, _: *mut c_void, _: *mut usize, _: *mut Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn locate_device_path_unimplemented (_: *mut Guid, _: *mut *mut device_path::Protocol, _: *mut Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn install_configuration_table_unimplemented (_: *mut Guid, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn load_image_unimplemented (_: Boolean, _: Handle, _: *mut device_path::Protocol, _: *mut c_void, _: usize, _: *mut Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn start_image_unimplemented (_: Handle, _: *mut usize, _: *mut *mut Char16) -> Status {
    unimplemented!()
  }}

  eficall! {fn exit_unimplemented (_: Handle, _: Status, _: usize, _: *mut Char16) -> Status {
    unimplemented!()
  }}

  eficall! {fn unload_image_unimplemented (_: Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn exit_boot_services_unimplemented (_: Handle, _: usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn get_next_monotonic_count_unimplemented (_: *mut u64) -> Status {
    unimplemented!()
  }}

  eficall! {fn stall_unimplemented (_: usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn set_watchdog_timer_unimplemented (_: usize, _: u64, _: usize, _: *mut Char16) -> Status {
    unimplemented!()
  }}

  eficall! {fn connect_controller_unimplemented (_: Handle, _: *mut Handle,  _: *mut device_path::Protocol, _: Boolean) -> Status {
    unimplemented!()
  }}

  eficall! {fn disconnect_controller_unimplemented (_: Handle, _: Handle, _: Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn open_protocol_unimplemented (_: Handle, _: *mut Guid, _: *mut *mut c_void, _: Handle, _: Handle, _: u32) -> Status {
    unimplemented!()
  }}

  eficall! {fn close_protocol_unimplemented (_: Handle, _: *mut Guid, _: Handle, _: Handle) -> Status {
    unimplemented!()
  }}

  eficall! {fn open_protocol_information_unimplemented (_: Handle, _: *mut Guid, _: *mut *mut r_efi::system::OpenProtocolInformationEntry, _: *mut usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn protocols_per_handle_unimplemented (_: Handle, _: *mut *mut *mut Guid, _: *mut usize) -> Status {
    unimplemented!()
  }}

  eficall! {fn locate_handle_buffer_unimplemented (_: r_efi::system::LocateSearchType, _: *mut Guid, _: *mut c_void, _: *mut usize, _: *mut *mut Handle) -> Status{
    unimplemented!()
  }}

  eficall! {fn locate_protocol_unimplemented (_: *mut Guid, _: *mut c_void, _: *mut *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn install_multiple_protocol_interfaces_unimplemented (_: *mut Handle, _: *mut c_void, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn uninstall_multiple_protocol_interfaces_unimplemented (_: *mut Handle, _: *mut c_void, _: *mut c_void) -> Status {
    unimplemented!()
  }}

  eficall! {fn calculate_crc32_unimplemented (_: *mut c_void, _: usize, _: *mut u32) -> Status {
    unimplemented!()
  }}

  eficall! {fn copy_mem_unimplemented (_: *mut c_void, _: *mut c_void, _: usize){
    unimplemented!()
  }}

  eficall! {fn set_mem_unimplemented (_: *mut c_void, _: usize, _: u8) {
    unimplemented!()
  }}

  eficall! {fn create_event_ex_unimplemented (_: u32, _: Tpl, _: Option<r_efi::system::EventNotify>, _: *const c_void, _: *const Guid, _: *mut Event) -> Status {
    unimplemented!()
  }}

  pub fn init_boot_services_table() -> EfiBootServicesTable {
    let mut bs = BootServices {
      hdr: TableHeader {
        signature: r_efi::system::BOOT_SERVICES_SIGNATURE,
        revision: r_efi::system::BOOT_SERVICES_REVISION,
        header_size: 0,
        crc32: 0,
        reserved: 0,
      },
      raise_tpl: Self::raise_tpl_unimplemented,
      restore_tpl: Self::restore_tpl_unimplemented,
      allocate_pages: Self::allocate_pages_unimplemented,
      free_pages: Self::free_pages_unimplemented,
      get_memory_map: Self::get_memory_map_unimplemented,
      allocate_pool: Self::allocate_pool_unimplemented,
      free_pool: Self::free_pool_unimplemented,
      create_event: Self::create_event_unimplemented,
      set_timer: Self::set_timer_unimplemented,
      wait_for_event: Self::wait_for_event_unimplemented,
      signal_event: Self::signal_event_unimplemented,
      close_event: Self::close_event_unimplemented,
      check_event: Self::check_event_unimplemented,
      install_protocol_interface: Self::install_protocol_interface_unimplemented,
      reinstall_protocol_interface: Self::reinstall_protocol_interface_unimplemented,
      uninstall_protocol_interface: Self::uninstall_protocol_interface_unimplemented,
      handle_protocol: Self::handle_protocol_unimplemented,
      reserved: 0 as *mut c_void,
      register_protocol_notify: Self::register_protocol_notify_unimplemented,
      locate_handle: Self::locate_handle_unimplemented,
      locate_device_path: Self::locate_device_path_unimplemented,
      install_configuration_table: Self::install_configuration_table_unimplemented,
      load_image: Self::load_image_unimplemented,
      start_image: Self::start_image_unimplemented,
      exit: Self::exit_unimplemented,
      unload_image: Self::unload_image_unimplemented,
      exit_boot_services: Self::exit_boot_services_unimplemented,
      get_next_monotonic_count: Self::get_next_monotonic_count_unimplemented,
      stall: Self::stall_unimplemented,
      set_watchdog_timer: Self::set_watchdog_timer_unimplemented,
      connect_controller: Self::connect_controller_unimplemented,
      disconnect_controller: Self::disconnect_controller_unimplemented,
      open_protocol: Self::open_protocol_unimplemented,
      close_protocol: Self::close_protocol_unimplemented,
      open_protocol_information: Self::open_protocol_information_unimplemented,
      protocols_per_handle: Self::protocols_per_handle_unimplemented,
      locate_handle_buffer: Self::locate_handle_buffer_unimplemented,
      locate_protocol: Self::locate_protocol_unimplemented,
      install_multiple_protocol_interfaces: Self::install_multiple_protocol_interfaces_unimplemented,
      uninstall_multiple_protocol_interfaces: Self::uninstall_multiple_protocol_interfaces_unimplemented,
      calculate_crc32: Self::calculate_crc32_unimplemented,
      copy_mem: Self::copy_mem_unimplemented,
      set_mem: Self::set_mem_unimplemented,
      create_event_ex: Self::create_event_ex_unimplemented,
    };

    bs.hdr.header_size = size_of::<BootServices>() as u32;
    let bs_ptr = &bs as *const BootServices as *const u8;
    let bs_slice = unsafe { from_raw_parts(bs_ptr, size_of::<RuntimeServices>()) };
    bs.hdr.crc32 = crc32fast::hash(bs_slice);

    EfiBootServicesTable { boot_services: Box::new(bs) }
  }
}

pub struct EfiSystemTable {
  system_table: Box<SystemTable, &'static dyn Allocator>,
  _boot_service: EfiBootServicesTable, // These fields ensure the BootServices and RuntimeServices structure pointers (in
  _runtime_service: EfiRuntimeServicesTable, // the system_table) have the same lifetime as the EfiSystemTable.
}

impl EfiSystemTable {
  pub fn init_system_table() -> EfiSystemTable {
    let mut st = SystemTable {
      hdr: TableHeader {
        signature: r_efi::system::SYSTEM_TABLE_SIGNATURE,
        revision: r_efi::system::SYSTEM_TABLE_REVISION,
        header_size: 0,
        crc32: 0,
        reserved: 0,
      },
      firmware_vendor: core::ptr::null_mut() as *mut u16,
      firmware_revision: 0,
      console_in_handle: core::ptr::null_mut() as *mut c_void,
      con_in: core::ptr::null_mut() as *mut simple_text_input::Protocol,
      console_out_handle: core::ptr::null_mut() as *mut c_void,
      con_out: core::ptr::null_mut() as *mut simple_text_output::Protocol,
      standard_error_handle: core::ptr::null_mut() as *mut c_void,
      std_err: core::ptr::null_mut() as *mut simple_text_output::Protocol,
      runtime_services: core::ptr::null_mut() as *mut RuntimeServices,
      boot_services: core::ptr::null_mut() as *mut BootServices,
      number_of_table_entries: 0,
      configuration_table: core::ptr::null_mut() as *mut r_efi::system::ConfigurationTable,
    };
    let mut bs = EfiBootServicesTable::init_boot_services_table();
    let mut rt = EfiRuntimeServicesTable::init_runtime_services_table();
    st.boot_services = bs.boot_services.as_mut();
    st.runtime_services = rt.runtime_services.as_mut();

    st.hdr.header_size = size_of::<SystemTable>() as u32;
    let st_ptr = &st as *const SystemTable as *const u8;
    let st_slice = unsafe { from_raw_parts(st_ptr, size_of::<SystemTable>()) };
    st.hdr.crc32 = crc32fast::hash(st_slice);

    EfiSystemTable {
      system_table: Box::new_in(st, &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR),
      _boot_service: bs,
      _runtime_service: rt,
    }
  }
  pub fn as_ref(&self) -> *const SystemTable {
    self.system_table.as_ref() as *const SystemTable
  }

  pub fn boot_services(&self) -> &mut BootServices {
    unsafe { self.system_table.boot_services.as_mut().expect("BootServices uninitialized") }
  }
}
