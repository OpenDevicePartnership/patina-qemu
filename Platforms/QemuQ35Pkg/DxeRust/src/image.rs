use core::{
  alloc::{AllocError, Allocator, Layout},
  ffi::c_void,
  mem::transmute,
  ptr::NonNull,
  slice::from_raw_parts,
};

use alloc::{alloc::Global, boxed::Box, collections::BTreeMap, string::String, vec::Vec};
use r_efi::system;
use r_pi::hob::{Hob, HobList};
use uefi_protocol_db_lib::DXE_CORE_HANDLE;
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

use crate::{
  allocator::{EFI_BOOT_SERVICES_CODE_ALLOCATOR, EFI_LOADER_CODE_ALLOCATOR, EFI_RUNTIME_SERVICES_CODE_ALLOCATOR},
  protocols::{core_install_protocol_interface, core_locate_device_path, PROTOCOL_DB},
  systemtables::EfiSystemTable,
};

use serial_print_dxe::println;

use corosensei::{
  stack::{Stack, StackPointer, MIN_STACK_SIZE, STACK_ALIGNMENT},
  Coroutine, CoroutineResult, Yielder,
};

#[cfg(windows)]
use corosensei::stack::StackTebFields;

pub const EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION: u16 = 10;
pub const EFI_IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER: u16 = 11;
pub const EFI_IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER: u16 = 12;

// Temporarily define this here until it is moved to r-efi.
pub(crate) mod hii_package_list {
  /// UEFI Specification, Release 2.10, Section 7.4.1 - EFI_BOOT_SERVICES.LoadImage() - Related Definitions
  pub(crate) const PROTOCOL_GUID: r_efi::efi::Guid =
    r_efi::efi::Guid::from_fields(0x6a1ee763, 0xd47a, 0x43b4, 0xaa, 0xbe, &[0xef, 0x1d, 0xe2, 0xab, 0x56, 0xfc]);
}

const ENTRY_POINT_STACK_SIZE: usize = 0x100000;

// dummy function used to initialize PrivateImageData.entry_point.
extern "efiapi" fn unimplemented_entry_point(
  _handle: r_efi::efi::Handle,
  _system_table: *mut r_efi::efi::SystemTable,
) -> r_efi::efi::Status {
  unimplemented!()
}

// dummy function used to initialize image_info.Unload.
extern "efiapi" fn unimplemented_unload(_handle: r_efi::efi::Handle) -> r_efi::efi::Status {
  r_efi::efi::Status::SUCCESS
}

// define a wrapper for allocators that supports specified alignments.
// this is needed to be able to do box allocations that respect image section
// alignments without resorting to calling unsafe allocator routines directly.
struct AlignedAllocWrapper(usize, &'static dyn Allocator);

impl AlignedAllocWrapper {
  fn new(alignment: usize, allocator: &'static dyn Allocator) -> Self {
    AlignedAllocWrapper(alignment, allocator)
  }
}

unsafe impl Allocator for AlignedAllocWrapper {
  fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    self.1.allocate(layout.align_to(self.0).unwrap())
  }
  unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
    self.1.deallocate(ptr, layout.align_to(self.0).unwrap())
  }
}
// define a stack structure for coroutine support.
struct ImageStack {
  stack: Box<[u8], AlignedAllocWrapper>,
}

impl ImageStack {
  fn new(size: usize) -> Self {
    ImageStack {
      stack: unsafe {
        Box::new_uninit_slice_in(size.max(MIN_STACK_SIZE), AlignedAllocWrapper::new(STACK_ALIGNMENT, &Global))
          .assume_init()
      },
    }
  }
}

unsafe impl Stack for ImageStack {
  fn base(&self) -> StackPointer {
    //stack grows downward, so "base" is the highest address, i.e. the ptr + size.
    self.limit().checked_add(self.stack.len()).unwrap()
  }
  fn limit(&self) -> StackPointer {
    //stack grows downward, so "limit" is the lowest address, i.e. the box ptr.
    StackPointer::new(self.stack.as_ref() as *const [u8] as *const u8 as usize).unwrap()
  }
  // these are required when building against windows target for testing
  #[cfg(windows)]
  fn teb_fields(&self) -> StackTebFields {
    unimplemented!()
  }

  #[cfg(windows)]
  fn update_teb_fields(&mut self, _: usize, _: usize) {
    unimplemented!()
  }
}

// This struct tracks private data associated with a particular image handle.
struct PrivateImageData {
  image_buffer: Box<[u8], AlignedAllocWrapper>,
  image_info: Box<r_efi::efi::protocols::loaded_image::Protocol>,
  hii_resource_section: Option<Box<[u8], AlignedAllocWrapper>>,
  image_type: u16,
  entry_point: r_efi::efi::ImageEntryPoint,
  filename: Option<String>,
  started: bool,
  exit_data: Option<(usize, *mut r_efi::efi::Char16)>,
  image_info_ptr: *mut c_void,
  image_device_path_ptr: *mut c_void,
}

impl PrivateImageData {
  fn new(image_info: r_efi::efi::protocols::loaded_image::Protocol) -> Self {
    PrivateImageData {
      image_buffer: Box::new_in([0; 0], AlignedAllocWrapper::new(1, &Global)),
      image_info: Box::new(image_info),
      hii_resource_section: None,
      image_type: 0,
      entry_point: unimplemented_entry_point,
      filename: None,
      started: false,
      exit_data: None,
      image_info_ptr: core::ptr::null_mut(),
      image_device_path_ptr: core::ptr::null_mut(),
    }
  }

  fn allocate_image(&mut self, size: usize, alignment: usize, allocator: &'static UefiAllocator) {
    let wrapped_allocator = AlignedAllocWrapper::new(alignment, allocator);

    self.image_buffer = unsafe { Box::new_uninit_slice_in(size, wrapped_allocator).assume_init() };
    self.image_info.image_base = self.image_buffer.as_mut_ptr() as *mut c_void;
  }

  fn allocate_resource_section(&mut self, size: usize, alignment: usize, allocator: &'static UefiAllocator) {
    let wrapped_allocator = AlignedAllocWrapper::new(alignment, allocator);

    self.hii_resource_section = Some(unsafe { Box::new_uninit_slice_in(size, wrapped_allocator).assume_init() });
  }
}

// This struct tracks global data used by the imaging subsystem.
struct DxeCoreGlobalImageData {
  dxe_core_image_handle: r_efi::efi::Handle,
  system_table: *mut r_efi::efi::SystemTable,
  private_image_data: BTreeMap<r_efi::efi::Handle, PrivateImageData>,
  current_running_image: Option<r_efi::efi::Handle>,
  image_start_contexts: Vec<*const Yielder<r_efi::efi::Handle, r_efi::efi::Status>>,
}

impl DxeCoreGlobalImageData {
  const fn new() -> Self {
    DxeCoreGlobalImageData {
      dxe_core_image_handle: core::ptr::null_mut(),
      system_table: core::ptr::null_mut() as *mut r_efi::efi::SystemTable,
      private_image_data: BTreeMap::<r_efi::efi::Handle, PrivateImageData>::new(),
      current_running_image: None,
      image_start_contexts: Vec::new(),
    }
  }
}

// DxeCoreGlobalImageData is accessed through a mutex guard, so it is safe to
// mark it sync/send.
unsafe impl Sync for DxeCoreGlobalImageData {}
unsafe impl Send for DxeCoreGlobalImageData {}

static PRIVATE_IMAGE_DATA: tpl_lock::TplMutex<DxeCoreGlobalImageData> =
  tpl_lock::TplMutex::new(system::TPL_NOTIFY, DxeCoreGlobalImageData::new(), "ImageLock");

// helper routine that returns an empty loaded_image::Protocol struct.
fn empty_image_info() -> r_efi::efi::protocols::loaded_image::Protocol {
  r_efi::efi::protocols::loaded_image::Protocol {
    revision: r_efi::efi::protocols::loaded_image::REVISION,
    parent_handle: core::ptr::null_mut(),
    system_table: core::ptr::null_mut(),
    device_handle: core::ptr::null_mut(),
    file_path: core::ptr::null_mut(),
    reserved: core::ptr::null_mut(),
    load_options_size: 0,
    load_options: core::ptr::null_mut(),
    image_base: core::ptr::null_mut(),
    image_size: 0,
    image_code_type: r_efi::efi::BOOT_SERVICES_CODE,
    image_data_type: r_efi::efi::BOOT_SERVICES_DATA,
    unload: unimplemented_unload,
  }
}

// retrieves the dxe core image info from the hob list, and installs the
// loaded_image protocol on it to create the dxe_core image handle.
fn install_dxe_core_image(hob_list: &HobList) {
  // Retrieve the MemoryAllocationModule hob corresponding to the DXE core
  // (i.e. this driver).
  let dxe_core_hob = hob_list
    .iter()
    .find_map(|x| if let Hob::MemoryAllocationModule(module) = x { Some(module) } else { None })
    .expect("Did not find MemoryAllocationModule Hob for DxeCore");

  // get exclusive access to the global private data.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();

  // convert the entry point from the hob into the appropriate function
  // pointer type and save it in the private_image_data structure for the core.
  // Safety: dxe_core_hob.entry_point must be the correct and actual entry
  // point for the core.
  let entry_point = unsafe { transmute(dxe_core_hob.entry_point) };

  // create the loaded_image structure for the core and populate it with data
  // from the hob.
  let mut image_info = empty_image_info();
  image_info.system_table = private_data.system_table;
  image_info.image_base = dxe_core_hob.alloc_descriptor.memory_base_address as *mut c_void;
  image_info.image_size = dxe_core_hob.alloc_descriptor.memory_length;

  let mut private_image_data = PrivateImageData::new(image_info);
  private_image_data.entry_point = entry_point;

  let image_info_ptr = private_image_data.image_info.as_ref() as *const r_efi::efi::protocols::loaded_image::Protocol;
  let image_info_ptr = image_info_ptr as *mut c_void;
  private_image_data.image_info_ptr = image_info_ptr;

  // install the loaded_image protocol on a new handle.
  let handle = match core_install_protocol_interface(
    Some(DXE_CORE_HANDLE),
    r_efi::efi::protocols::loaded_image::PROTOCOL_GUID,
    image_info_ptr,
  ) {
    Err(err) => panic!("Failed to install dxe_rust core image handle: {:?}", err),
    Ok(handle) => handle,
  };
  assert_eq!(handle, DXE_CORE_HANDLE);
  // record this handle as the new dxe_core handle.
  private_data.dxe_core_image_handle = handle;

  // store the dxe_core image private data in the private image data map.
  private_data.private_image_data.insert(handle, private_image_data);
}

// loads and relocates the image in the specified slice and returns the
// associated PrivateImageData structures.
fn core_load_pe_image(
  image: &[u8],
  mut image_info: r_efi::efi::protocols::loaded_image::Protocol,
) -> Result<PrivateImageData, r_efi::efi::Status> {
  // parse and validate the header and retrieve the image data from it.
  let pe_info = uefi_pe32_lib::pe32_get_image_info(image).map_err(|_| r_efi::efi::Status::UNSUPPORTED)?;

  // based on the image type, determine the correct allocator and code/data types.
  let (allocator, code_type, data_type) = match pe_info.image_type {
    EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION => {
      (&EFI_LOADER_CODE_ALLOCATOR, r_efi::efi::LOADER_CODE, r_efi::efi::LOADER_DATA)
    }
    EFI_IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER => {
      (&EFI_BOOT_SERVICES_CODE_ALLOCATOR, r_efi::efi::BOOT_SERVICES_CODE, r_efi::efi::BOOT_SERVICES_DATA)
    }
    EFI_IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER => {
      (&EFI_RUNTIME_SERVICES_CODE_ALLOCATOR, r_efi::efi::RUNTIME_SERVICES_CODE, r_efi::efi::RUNTIME_SERVICES_DATA)
    }
    _ => return Err(r_efi::efi::Status::UNSUPPORTED),
  };

  let alignment = pe_info.section_alignment as usize;
  let size = pe_info.size_of_image as usize;

  image_info.image_size = size as u64;
  image_info.image_code_type = code_type;
  image_info.image_data_type = data_type;

  let mut private_info = PrivateImageData::new(image_info);
  private_info.filename = pe_info.filename.clone();
  private_info.image_type = pe_info.image_type;

  //allocate a buffer to hold the image (also updates private_info.image_info.image_base)
  private_info.allocate_image(size, alignment, &allocator);
  let loaded_image = &mut private_info.image_buffer;

  //load the image into the new loaded image buffer
  uefi_pe32_lib::pe32_load_image(image, loaded_image).map_err(|_| r_efi::efi::Status::LOAD_ERROR)?;

  //relocate the image to the address at which it was loaded.
  let loaded_image_addr = private_info.image_info.image_base as usize;
  uefi_pe32_lib::pe32_relocate_image(loaded_image_addr, loaded_image).map_err(|_| r_efi::efi::Status::LOAD_ERROR)?;

  // update the entry point. Transmute is required here to cast the raw function address to the ImageEntryPoint function pointer type.
  private_info.entry_point = unsafe { transmute(loaded_image_addr + pe_info.entry_point_offset) };

  let result = uefi_pe32_lib::pe32_load_resource_section(image).map_err(|_| r_efi::efi::Status::LOAD_ERROR)?;

  if let Some(resource_section) = result {
    private_info.allocate_resource_section(resource_section.len(), alignment, &allocator);
    private_info.hii_resource_section.as_mut().unwrap().copy_from_slice(resource_section);
    println!("HII Resource Section found for {}.", pe_info.filename.as_deref().unwrap_or("Unknown"));
  }

  Ok(private_info)
}

/// Loads the image specified by the device path (not yet supported) or slice.
/// * parent_image_handle - the handle of the image that is loading this one.
/// * device_path - optional device path describing where to load the image from
///                 (not yet supported).
/// * image - optional slice containing the image data.
///
/// One of `device_path` or `image` must be specified.
/// returns the image handle of the freshly loaded image.
pub fn core_load_image(
  parent_image_handle: r_efi::efi::Handle,
  device_path: *mut r_efi::efi::protocols::device_path::Protocol,
  image: Option<&[u8]>,
) -> Result<r_efi::efi::Handle, r_efi::efi::Status> {
  PROTOCOL_DB.validate_handle(parent_image_handle)?;

  if image.is_none() && device_path.is_null() {
    return Err(r_efi::efi::Status::INVALID_PARAMETER);
  }

  //TODO: we only support loading from a slice for now. device path support
  //needs to be added.
  if image.is_none() {
    todo!("Support for loading from DevicePath within LoadImage not yet implemented.");
  }

  let image = image.unwrap();

  //TODO: image authentication not implemented yet.

  // load the image.
  let mut image_info = empty_image_info();
  image_info.system_table = PRIVATE_IMAGE_DATA.lock().system_table;
  image_info.parent_handle = parent_image_handle;

  if !device_path.is_null() {
    if let Ok((_, handle)) = core_locate_device_path(r_efi::protocols::device_path::PROTOCOL_GUID, device_path) {
      image_info.device_handle = handle;
    }
  }

  let mut private_info = core_load_pe_image(image, image_info)?;

  let image_info_ptr = private_info.image_info.as_ref() as *const r_efi::efi::protocols::loaded_image::Protocol;
  let image_info_ptr = image_info_ptr as *mut c_void;

  println!(
    "Loaded driver at {:#x?} EntryPoint={:#x?} {:}",
    private_info.image_info.image_base,
    private_info.entry_point as usize,
    private_info.filename.as_ref().unwrap_or(&String::from("<no PDB>"))
  );

  // install the loaded_image protocol for this freshly loaded image on a new
  // handle.
  let handle =
    core_install_protocol_interface(None, r_efi::efi::protocols::loaded_image::PROTOCOL_GUID, image_info_ptr)?;

  // install the loaded_image device path protocol for the new image. Note: device_path might be null,
  // in which case a null interface is installed as a marker.
  core_install_protocol_interface(
    Some(handle),
    r_efi::efi::protocols::loaded_image_device_path::PROTOCOL_GUID,
    device_path as *mut c_void,
  )?;

  if let Some(res_section) = &private_info.hii_resource_section {
    core_install_protocol_interface(
      Some(handle),
      hii_package_list::PROTOCOL_GUID,
      res_section.as_ref().as_ptr() as *mut c_void,
    )?;
  }

  // Store the interface pointers for unload to use when uninstalling these protocol interfaces.
  private_info.image_info_ptr = image_info_ptr;
  private_info.image_device_path_ptr = device_path as *mut c_void;

  // save the private image data for this image in the private image data map.
  PRIVATE_IMAGE_DATA.lock().private_image_data.insert(handle, private_info);

  // return the new handle.
  Ok(handle)
}

// Loads the image specified by the device_path (not yet supported) or
// source_buffer argument. See EFI_BOOT_SERVICES::LoadImage() API definition
// in UEFI spec for usage details.
// * boot_policy - indicates whether the image is being loaded by the boot
//                 manager from the specified device path. ignored if
//                 source_buffer is not null.
// * parent_image_handle - the caller's image handle.
// * device_path - the file path from which the image is loaded.
// * source_buffer - if not null, pointer to the memory location containing the
//                   image to be loaded.
//  * source_size - size in bytes of source_buffer. ignored if source_buffer is
//                  null.
//  * image_handle - pointer to the returned image handle that is created on
//                   successful image load.
extern "efiapi" fn load_image(
  _boot_policy: r_efi::efi::Boolean,
  parent_image_handle: r_efi::efi::Handle,
  device_path: *mut r_efi::efi::protocols::device_path::Protocol,
  source_buffer: *mut c_void,
  source_size: usize,
  image_handle: *mut r_efi::efi::Handle,
) -> r_efi::efi::Status {
  if image_handle.is_null() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  let image = if source_buffer.is_null() {
    None
  } else {
    Some(unsafe { from_raw_parts(source_buffer as *const u8, source_size) })
  };

  match core_load_image(parent_image_handle, device_path, image) {
    Err(err) => return err,
    Ok(handle) => unsafe { image_handle.write(handle) },
  }

  r_efi::efi::Status::SUCCESS
}

// Transfers control to the entry point of an image that was loaded by
// load_image. See EFI_BOOT_SERVICES::StartImage() API definition in UEFI spec
// for usage details.
// * image_handle - handle of the image to be started.
// * exit_data_size - pointer to receive the size, in bytes, of exit_data.
//                    if exit_data is null, this is parameter is ignored.
// * exit_data - pointer to receive a data buffer with exit data, if any.
pub extern "efiapi" fn start_image(
  image_handle: r_efi::efi::Handle,
  exit_data_size: *mut usize,
  exit_data: *mut *mut r_efi::efi::Char16,
) -> r_efi::efi::Status {
  // allocate a buffer for the entry point stack.
  let stack = ImageStack::new(ENTRY_POINT_STACK_SIZE);

  // define a co-routine that wraps the entry point execution. this doesn't
  // run until the coroutine.resume() call below.
  let mut coroutine = Coroutine::with_stack(stack, move |yielder, image_handle| {
    let mut private_data = PRIVATE_IMAGE_DATA.lock();

    // mark the image as started and grab a copy of the private info.
    let private_info = private_data.private_image_data.get_mut(&image_handle).unwrap();
    private_info.started = true;
    let entry_point = private_info.entry_point;

    // save a pointer to the yeilder so that exit() can use it.
    private_data.image_start_contexts.push(yielder as *const Yielder<_, _>);

    // get a copy of the system table pointer to pass to the entry point.
    let system_table = private_data.system_table;
    // drop our reference to the private data (i.e. release the lock).
    drop(private_data);

    // invoke the entry point. Code on the other side of this pointer is
    // FFI, which is inherently unsafe, but it's not  "technically" unsafe
    // from a rust standpoint since r_efi doesn't define the ImageEntryPoint
    // pointer type as "pointer to unsafe function"
    let status = entry_point(image_handle, system_table);

    //safety note: any variables with "Drop" routines that need to run
    //need to be explicitly dropped before calling exit(). Since exit()
    //effectively "longjmps" back to StartImage(), rust automatic
    //drops will not be triggered.
    exit(image_handle, status, 0, core::ptr::null_mut());
    status
  });

  // Save the handle of the previously running image and update the currently
  // running image to the one we are about to invoke. In the event of nested
  // calls to StartImage(), the chain of previously running images will
  // be preserved on the stack of the various StartImage() instances.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  let previous_image = private_data.current_running_image;
  let image_type = private_data.private_image_data.get_mut(&image_handle).unwrap().image_type;
  private_data.current_running_image = Some(image_handle);
  drop(private_data);

  // switch stacks and execute the above defined coroutine to start the image.
  let status = match coroutine.resume(image_handle) {
    CoroutineResult::Yield(status) => status,
    // Note: `CoroutineResult::Return` is unexpected, since it would imply
    // that exit() failed. TODO: should panic here?
    CoroutineResult::Return(status) => status,
  };

  // because we used exit() to return from the coroutine (as opposed to
  // returning naturally from it), the coroutine is marked as suspended rather
  // than complete. We need to forcibly mark the coroutine done; otherwise it
  // will try to use unwind to clean up the co-routine stack (i.e. "drop" any
  // live objects). This unwind support requires std and will panic if
  // executed.
  unsafe { coroutine.force_reset() };

  PRIVATE_IMAGE_DATA.lock().current_running_image = previous_image;

  // retrieve any exit data that was provided by the entry point.
  if !exit_data_size.is_null() && !exit_data.is_null() {
    let private_data = PRIVATE_IMAGE_DATA.lock();
    let image_data = private_data.private_image_data.get(&image_handle).unwrap();
    if let Some(image_exit_data) = image_data.exit_data {
      unsafe {
        exit_data_size.write(image_exit_data.0);
        exit_data.write(image_exit_data.1);
      }
    }
  }

  if status != r_efi::efi::Status::SUCCESS || image_type == EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION {
    let _result = core_unload_image(image_handle, true);
  }

  status
}

pub fn core_unload_image(image_handle: r_efi::efi::Handle, force_unload: bool) -> r_efi::efi::Status {
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  let private_image_data = match private_data.private_image_data.get(&image_handle) {
    Some(data) => data,
    None => return r_efi::efi::Status::INVALID_PARAMETER,
  };

  // if the image has been started, request that it unload, and don't unload it if
  // the unload function doesn't exist or returns an error.
  if private_image_data.started {
    if (private_image_data.image_info.unload as *const c_void) != core::ptr::null_mut() {
      //Safety: this is unsafe (even though rust doesn't think so) because we are calling
      //into the "unload" function pointer that the image itself set. r_efi doesn't mark
      //the unload function type as unsafe - so rust reports an "unused_unsafe" since it
      //doesn't know it's unsafe. We suppress the warning and mark it unsafe anyway as a
      //warning to the future.
      #[allow(unused_unsafe)]
      unsafe {
        let status = (private_image_data.image_info.unload)(image_handle);
        if status != r_efi::efi::Status::SUCCESS {
          return status;
        }
      }
    } else {
      if !force_unload {
        return r_efi::efi::Status::UNSUPPORTED;
      }
    }
  }

  let handles = match PROTOCOL_DB.locate_handles(None) {
    Err(err) => return err,
    Ok(handles) => handles,
  };

  // close any protocols opened by this image.
  for handle in handles {
    let protocols = match PROTOCOL_DB.get_protocols_on_handle(handle) {
      Err(_) => continue,
      Ok(protocols) => protocols,
    };
    for protocol in protocols {
      let open_infos = match PROTOCOL_DB.get_open_protocol_information_by_protocol(handle, protocol) {
        Err(_) => continue,
        Ok(open_infos) => open_infos,
      };
      for open_info in open_infos {
        if Some(image_handle) == open_info.agent_handle {
          let _result =
            PROTOCOL_DB.remove_protocol_usage(handle, protocol, open_info.agent_handle, open_info.controller_handle);
        }
      }
    }
  }

  // remove the private data for this image from the private_image_data map.
  // it will get dropped when it goes out of scope at the end of the function,
  // and the image and image_info boxes along with it.
  let private_image_data = private_data.private_image_data.remove(&image_handle).unwrap();
  // remove the image and device path protocols from the image handle.
  match PROTOCOL_DB.uninstall_protocol_interface(
    image_handle,
    r_efi::efi::protocols::loaded_image::PROTOCOL_GUID,
    private_image_data.image_info_ptr,
  ) {
    Err(err) => return err,
    _ => (),
  };
  match PROTOCOL_DB.uninstall_protocol_interface(
    image_handle,
    r_efi::efi::protocols::loaded_image_device_path::PROTOCOL_GUID,
    private_image_data.image_device_path_ptr,
  ) {
    Err(err) => return err,
    _ => (),
  };

  r_efi::efi::Status::SUCCESS
}

extern "efiapi" fn unload_image(image_handle: r_efi::efi::Handle) -> r_efi::efi::Status {
  core_unload_image(image_handle, false)
}

// Terminates a loaded EFI image and returns control to boot services.
// See EFI_BOOT_SERVICES::Exit() API definition in UEFI spec for usage details.
// * image_handle - the handle of the currently running image.
// * exit_status - the exit status for the image.
// * exit_data_size - the size of the exit_data buffer, if exit_data is not
//                    null.
// * exit_data - optional buffer of data provided by the caller.
extern "efiapi" fn exit(
  image_handle: r_efi::efi::Handle,
  status: r_efi::efi::Status,
  exit_data_size: usize,
  exit_data: *mut r_efi::efi::Char16,
) -> r_efi::efi::Status {
  // check the currently running image.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  if Some(image_handle) != private_data.current_running_image {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  // save the exit data, if present, into the private_image_data for this
  // image for start_image to retrieve and return.
  if (exit_data_size != 0) && !exit_data.is_null() {
    let image_data = private_data.private_image_data.get_mut(&image_handle).unwrap();
    image_data.exit_data = Some((exit_data_size, exit_data));
  }

  // retrieve the yielder that was saved in the start_image entry point
  // coroutine wrapper.
  // safety note: this assumes that the top of the image_start_contexts stack
  // is the currently running image.
  let yielder = private_data.image_start_contexts.pop().unwrap();
  let yielder = unsafe { &*yielder };
  drop(private_data);

  // safety note: any variables with "Drop" routines that need to run
  // need to be explicitly dropped before calling suspend(). Since suspend()
  // effectively "longjmps" back to StartImage(), rust automatic
  // drops will not be triggered.

  // transfer control back to start_image by calling the suspend function on
  // yielder. This will switch stacks back to the start_image that invoked
  // the entry point coroutine.
  yielder.suspend(status);

  //should never reach here, but rust doesn't know that.
  r_efi::efi::Status::ACCESS_DENIED
}

/// Initializes image services for the DXE core.
pub fn init_image_support(hob_list: &HobList, system_table: &EfiSystemTable) {
  // initialize system table entry in private global.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  private_data.system_table = system_table.as_ptr() as *mut r_efi::efi::SystemTable;
  drop(private_data);

  // install the image protocol for the dxe_core.
  install_dxe_core_image(hob_list);

  //set up imaging services
  system_table.boot_services().load_image = load_image;
  system_table.boot_services().start_image = start_image;
  system_table.boot_services().unload_image = unload_image;
  system_table.boot_services().exit = exit;
}
