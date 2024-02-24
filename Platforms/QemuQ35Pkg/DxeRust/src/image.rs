use core::{
  alloc::{AllocError, Allocator, Layout},
  ffi::c_void,
  mem::transmute,
  ptr::NonNull,
  slice::from_raw_parts,
};

use alloc::{alloc::Global, boxed::Box, collections::BTreeMap, string::String, vec, vec::Vec};
use r_efi::efi;
use r_pi::hob::{Hob, HobList};
use uefi_device_path_lib::{copy_device_path_to_boxed_slice, device_path_node_count, DevicePathWalker};
use uefi_protocol_db_lib::DXE_CORE_HANDLE;
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

use crate::{
  allocator::{EFI_BOOT_SERVICES_CODE_ALLOCATOR, EFI_LOADER_CODE_ALLOCATOR, EFI_RUNTIME_SERVICES_CODE_ALLOCATOR},
  filesystems::SimpleFile,
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

//TODO: the following protocol definitions should be pushed to r_efi
const EFI_LOAD_FILE_PROTOCOL: efi::Guid =
  efi::Guid::from_fields(0x56EC3091, 0x954C, 0x11d2, 0x8e, 0x3f, &[0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);

const EFI_LOAD_FILE2_PROTOCOL: efi::Guid =
  efi::Guid::from_fields(0x4006c0c1, 0xfcb3, 0x403e, 0x99, 0x6d, &[0x4a, 0x6c, 0x87, 0x24, 0xe0, 0x6d]);

type LoadFile = extern "efiapi" fn(
  this: *mut LoadFileProtocol,
  file_path: *mut efi::protocols::device_path::Protocol,
  boot_policy: bool,
  buffer_size: *mut usize,
  buffer: *mut c_void,
) -> efi::Status;

struct LoadFileProtocol {
  load_file: LoadFile,
}
//end r_efi protocols

pub const EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION: u16 = 10;
pub const EFI_IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER: u16 = 11;
pub const EFI_IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER: u16 = 12;

const ENTRY_POINT_STACK_SIZE: usize = 0x100000;

// dummy function used to initialize PrivateImageData.entry_point.
extern "efiapi" fn unimplemented_entry_point(
  _handle: efi::Handle,
  _system_table: *mut efi::SystemTable,
) -> efi::Status {
  unimplemented!()
}

// dummy function used to initialize image_info.Unload.
extern "efiapi" fn unimplemented_unload(_handle: efi::Handle) -> efi::Status {
  unimplemented!("unimplemented unload should never be called.")
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
  image_info: Box<efi::protocols::loaded_image::Protocol>,
  hii_resource_section: Option<Box<[u8], AlignedAllocWrapper>>,
  image_type: u16,
  entry_point: efi::ImageEntryPoint,
  filename: Option<String>,
  started: bool,
  exit_data: Option<(usize, *mut efi::Char16)>,
  image_info_ptr: *mut c_void,
  image_device_path_ptr: *mut c_void,
}

impl PrivateImageData {
  fn new(image_info: efi::protocols::loaded_image::Protocol) -> Self {
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
  dxe_core_image_handle: efi::Handle,
  system_table: *mut efi::SystemTable,
  private_image_data: BTreeMap<efi::Handle, PrivateImageData>,
  current_running_image: Option<efi::Handle>,
  image_start_contexts: Vec<*const Yielder<efi::Handle, efi::Status>>,
}

impl DxeCoreGlobalImageData {
  const fn new() -> Self {
    DxeCoreGlobalImageData {
      dxe_core_image_handle: core::ptr::null_mut(),
      system_table: core::ptr::null_mut(),
      private_image_data: BTreeMap::<efi::Handle, PrivateImageData>::new(),
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
  tpl_lock::TplMutex::new(efi::TPL_NOTIFY, DxeCoreGlobalImageData::new(), "ImageLock");

// helper routine that returns an empty loaded_image::Protocol struct.
fn empty_image_info() -> efi::protocols::loaded_image::Protocol {
  efi::protocols::loaded_image::Protocol {
    revision: efi::protocols::loaded_image::REVISION,
    parent_handle: core::ptr::null_mut(),
    system_table: core::ptr::null_mut(),
    device_handle: core::ptr::null_mut(),
    file_path: core::ptr::null_mut(),
    reserved: core::ptr::null_mut(),
    load_options_size: 0,
    load_options: core::ptr::null_mut(),
    image_base: core::ptr::null_mut(),
    image_size: 0,
    image_code_type: efi::BOOT_SERVICES_CODE,
    image_data_type: efi::BOOT_SERVICES_DATA,
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

  let image_info_ptr = private_image_data.image_info.as_ref() as *const efi::protocols::loaded_image::Protocol;
  let image_info_ptr = image_info_ptr as *mut c_void;
  private_image_data.image_info_ptr = image_info_ptr;

  // install the loaded_image protocol on a new handle.
  let handle = match core_install_protocol_interface(
    Some(DXE_CORE_HANDLE),
    efi::protocols::loaded_image::PROTOCOL_GUID,
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
  mut image_info: efi::protocols::loaded_image::Protocol,
) -> Result<PrivateImageData, efi::Status> {
  // parse and validate the header and retrieve the image data from it.
  let pe_info = uefi_pe32_lib::pe32_get_image_info(image).map_err(|_| efi::Status::UNSUPPORTED)?;

  // based on the image type, determine the correct allocator and code/data types.
  let (allocator, code_type, data_type) = match pe_info.image_type {
    EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION => (&EFI_LOADER_CODE_ALLOCATOR, efi::LOADER_CODE, efi::LOADER_DATA),
    EFI_IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER => {
      (&EFI_BOOT_SERVICES_CODE_ALLOCATOR, efi::BOOT_SERVICES_CODE, efi::BOOT_SERVICES_DATA)
    }
    EFI_IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER => {
      (&EFI_RUNTIME_SERVICES_CODE_ALLOCATOR, efi::RUNTIME_SERVICES_CODE, efi::RUNTIME_SERVICES_DATA)
    }
    _ => return Err(efi::Status::UNSUPPORTED),
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
  private_info.allocate_image(size, alignment, allocator);
  let loaded_image = &mut private_info.image_buffer;

  //load the image into the new loaded image buffer
  uefi_pe32_lib::pe32_load_image(image, loaded_image).map_err(|_| efi::Status::LOAD_ERROR)?;

  //relocate the image to the address at which it was loaded.
  let loaded_image_addr = private_info.image_info.image_base as usize;
  uefi_pe32_lib::pe32_relocate_image(loaded_image_addr, loaded_image).map_err(|_| efi::Status::LOAD_ERROR)?;

  // update the entry point. Transmute is required here to cast the raw function address to the ImageEntryPoint function pointer type.
  private_info.entry_point = unsafe { transmute(loaded_image_addr + pe_info.entry_point_offset) };

  let result = uefi_pe32_lib::pe32_load_resource_section(image).map_err(|_| efi::Status::LOAD_ERROR)?;

  if let Some((resource_section_offset, resource_section_size)) = result {
    private_info.allocate_resource_section(resource_section_size, alignment, &allocator);
    private_info.hii_resource_section.as_mut().unwrap().copy_from_slice(
      &private_info.image_buffer[resource_section_offset..resource_section_offset + resource_section_size],
    );
    println!("HII Resource Section found for {}.", pe_info.filename.as_deref().unwrap_or("Unknown"));
  }

  Ok(private_info)
}

fn get_buffer_by_file_path(
  boot_policy: bool,
  file_path: *mut efi::protocols::device_path::Protocol,
) -> Result<Vec<u8>, efi::Status> {
  if file_path.is_null() {
    Err(efi::Status::INVALID_PARAMETER)?;
  }
  if let Ok(buffer) = get_file_buffer_from_sfs(file_path) {
    return Ok(buffer);
  }

  if boot_policy {
    if let Ok(buffer) = get_file_buffer_from_load_protocol(EFI_LOAD_FILE2_PROTOCOL, false, file_path) {
      return Ok(buffer);
    }
  }

  if let Ok(buffer) = get_file_buffer_from_load_protocol(EFI_LOAD_FILE_PROTOCOL, boot_policy, file_path) {
    return Ok(buffer);
  }

  Err(efi::Status::NOT_FOUND)
}

fn get_file_buffer_from_sfs(file_path: *mut efi::protocols::device_path::Protocol) -> Result<Vec<u8>, efi::Status> {
  let (remaining_file_path, handle) =
    core_locate_device_path(efi::protocols::simple_file_system::PROTOCOL_GUID, file_path)?;

  let mut file = SimpleFile::open_volume(handle)?;

  for node in unsafe { DevicePathWalker::new(remaining_file_path) } {
    match node.header.r#type {
      //TODO: Subtype MEDIA_FILE_PATH_DP = 4 not defined in r_efi yet.
      efi::protocols::device_path::TYPE_MEDIA if node.header.sub_type == 4 => (), //proceed on valid path node
      efi::protocols::device_path::TYPE_END => break,
      _ => Err(efi::Status::UNSUPPORTED)?,
    }
    //For MEDIA_FILE_PATH_DP, file name is in the node data, but it needs to be converted to Vec<u16> for call to open.
    let filename: Vec<u16> = node.data.chunks_exact(2).map(|x| u16::from_le_bytes(x.try_into().unwrap())).collect();

    file = file.open(filename, efi::protocols::file::MODE_READ, 0)?;
  }

  // if execution comes here, the above loop was successfully able to open all the files on the remaining device path,
  // so `file` is currently pointing to the desired file (i.e. the last node), and it just needs to be read.
  file.read()
}

fn get_file_buffer_from_load_protocol(
  protocol: efi::Guid,
  boot_policy: bool,
  file_path: *mut efi::protocols::device_path::Protocol,
) -> Result<Vec<u8>, efi::Status> {
  if !(protocol == EFI_LOAD_FILE_PROTOCOL || protocol == EFI_LOAD_FILE2_PROTOCOL) {
    Err(efi::Status::INVALID_PARAMETER)?;
  }

  if protocol == EFI_LOAD_FILE2_PROTOCOL && boot_policy {
    Err(efi::Status::INVALID_PARAMETER)?;
  }

  let (remaining_file_path, handle) = core_locate_device_path(protocol, file_path)?;

  let load_file = PROTOCOL_DB.get_interface_for_handle(handle, protocol)?;
  let load_file = unsafe { (load_file as *mut LoadFileProtocol).as_mut().ok_or(efi::Status::UNSUPPORTED)? };

  //determine buffer size.
  let mut buffer_size = 0;
  match (load_file.load_file)(
    load_file,
    remaining_file_path,
    boot_policy,
    core::ptr::addr_of_mut!(buffer_size),
    core::ptr::null_mut(),
  ) {
    efi::Status::BUFFER_TOO_SMALL => (),                     //expected
    efi::Status::SUCCESS => Err(efi::Status::DEVICE_ERROR)?, //not expected for buffer_size = 0
    err => Err(err)?,                                        //unexpected error.
  }

  let mut file_buffer = vec![0u8; buffer_size];
  match (load_file.load_file)(
    load_file,
    remaining_file_path,
    boot_policy,
    core::ptr::addr_of_mut!(buffer_size),
    file_buffer.as_mut_ptr() as *mut c_void,
  ) {
    efi::Status::SUCCESS => Ok(file_buffer),
    err => Err(err),
  }
}

/// Loads the image specified by the device path (not yet supported) or slice.
/// * parent_image_handle - the handle of the image that is loading this one.
/// * device_path - optional device path describing where to load the image from.
/// * image - optional slice containing the image data.
///
/// One of `device_path` or `image` must be specified.
/// returns the image handle of the freshly loaded image.
pub fn core_load_image(
  boot_policy: bool,
  parent_image_handle: efi::Handle,
  device_path: *mut efi::protocols::device_path::Protocol,
  image: Option<&[u8]>,
) -> Result<efi::Handle, efi::Status> {
  if image.is_none() && device_path.is_null() {
    return Err(efi::Status::INVALID_PARAMETER);
  }

  PROTOCOL_DB.validate_handle(parent_image_handle)?;
  PROTOCOL_DB
    .get_interface_for_handle(parent_image_handle, efi::protocols::loaded_image::PROTOCOL_GUID)
    .map_err(|_| efi::Status::INVALID_PARAMETER)?;

  let image_to_load = match image {
    Some(image) => image.to_vec(),
    None => get_buffer_by_file_path(boot_policy, device_path)?,
  };

  //TODO: image authentication not implemented yet.

  // load the image.
  let mut image_info = empty_image_info();
  image_info.system_table = PRIVATE_IMAGE_DATA.lock().system_table;
  image_info.parent_handle = parent_image_handle;

  if !device_path.is_null() {
    if let Ok((_, handle)) = core_locate_device_path(efi::protocols::device_path::PROTOCOL_GUID, device_path) {
      image_info.device_handle = handle;
    }

    // extract file path here and set in image_info
    let (_, device_path_size) = device_path_node_count(device_path);
    let file_path_size: usize =
      device_path_size.saturating_sub(core::mem::size_of::<efi::protocols::device_path::Protocol>());
    let file_path = unsafe { (device_path as *const u8).offset(file_path_size as isize) };
    image_info.file_path = file_path as *mut efi::protocols::device_path::Protocol;
  }

  let mut private_info = core_load_pe_image(image_to_load.as_ref(), image_info)?;

  let image_info_ptr = private_info.image_info.as_ref() as *const efi::protocols::loaded_image::Protocol;
  let image_info_ptr = image_info_ptr as *mut c_void;

  println!(
    "Loaded driver at {:#x?} EntryPoint={:#x?} {:}",
    private_info.image_info.image_base,
    private_info.entry_point as usize,
    private_info.filename.as_ref().unwrap_or(&String::from("<no PDB>"))
  );

  // install the loaded_image protocol for this freshly loaded image on a new
  // handle.
  let handle = core_install_protocol_interface(None, efi::protocols::loaded_image::PROTOCOL_GUID, image_info_ptr)?;

  // install the loaded_image device path protocol for the new image. If input device path is not null, then make a
  // permanent copy on the heap.
  let loaded_image_device_path = if device_path.is_null() {
    core::ptr::null_mut()
  } else {
    // make copy and convert to raw pointer to avoid drop at end of function.
    Box::into_raw(copy_device_path_to_boxed_slice(device_path)) as *mut u8
  };

  core_install_protocol_interface(
    Some(handle),
    efi::protocols::loaded_image_device_path::PROTOCOL_GUID,
    loaded_image_device_path as *mut c_void,
  )?;

  if let Some(res_section) = &private_info.hii_resource_section {
    core_install_protocol_interface(
      Some(handle),
      efi::protocols::hii_package_list::PROTOCOL_GUID,
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
  boot_policy: efi::Boolean,
  parent_image_handle: efi::Handle,
  device_path: *mut efi::protocols::device_path::Protocol,
  source_buffer: *mut c_void,
  source_size: usize,
  image_handle: *mut efi::Handle,
) -> efi::Status {
  if image_handle.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  let image = if source_buffer.is_null() {
    None
  } else {
    if source_size == 0 {
      return efi::Status::LOAD_ERROR;
    }
    Some(unsafe { from_raw_parts(source_buffer as *const u8, source_size) })
  };

  match core_load_image(boot_policy.into(), parent_image_handle, device_path, image) {
    Err(err) => return err,
    Ok(handle) => unsafe { image_handle.write(handle) },
  }

  efi::Status::SUCCESS
}

// Transfers control to the entry point of an image that was loaded by
// load_image. See EFI_BOOT_SERVICES::StartImage() API definition in UEFI spec
// for usage details.
// * image_handle - handle of the image to be started.
// * exit_data_size - pointer to receive the size, in bytes, of exit_data.
//                    if exit_data is null, this is parameter is ignored.
// * exit_data - pointer to receive a data buffer with exit data, if any.
extern "efiapi" fn start_image(
  image_handle: efi::Handle,
  exit_data_size: *mut usize,
  exit_data: *mut *mut efi::Char16,
) -> efi::Status {
  let status = core_start_image(image_handle);

  // retrieve any exit data that was provided by the entry point.
  if !exit_data_size.is_null() && !exit_data.is_null() {
    let private_data = PRIVATE_IMAGE_DATA.lock();
    if let Some(image_data) = private_data.private_image_data.get(&image_handle) {
      if let Some(image_exit_data) = image_data.exit_data {
        unsafe {
          exit_data_size.write(image_exit_data.0);
          exit_data.write(image_exit_data.1);
        }
      }
    }
  }

  let image_type = PRIVATE_IMAGE_DATA.lock().private_image_data.get(&image_handle).map(|x| x.image_type);

  if status.is_err() || image_type == Some(EFI_IMAGE_SUBSYSTEM_EFI_APPLICATION) {
    let _result = core_unload_image(image_handle, true);
  }

  match status {
    Ok(()) => efi::Status::SUCCESS,
    Err(err) => err,
  }
}

pub fn core_start_image(image_handle: efi::Handle) -> Result<(), efi::Status> {
  PROTOCOL_DB.validate_handle(image_handle)?;

  if let Some(private_data) = PRIVATE_IMAGE_DATA.lock().private_image_data.get_mut(&image_handle) {
    if private_data.started {
      Err(efi::Status::INVALID_PARAMETER)?;
    }
  } else {
    Err(efi::Status::INVALID_PARAMETER)?;
  }

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
  match status {
    efi::Status::SUCCESS => Ok(()),
    err => Err(err),
  }
}

pub fn core_unload_image(image_handle: efi::Handle, force_unload: bool) -> Result<(), efi::Status> {
  PROTOCOL_DB.validate_handle(image_handle)?;
  let private_data = PRIVATE_IMAGE_DATA.lock();
  let private_image_data = private_data.private_image_data.get(&image_handle).ok_or(efi::Status::INVALID_PARAMETER)?;
  let unload_function = Option::from(private_image_data.image_info.unload);
  let started = private_image_data.started;
  drop(private_data); // release the image lock while unload logic executes as this function may be re-entrant.

  // if the image has been started, request that it unload, and don't unload it if
  // the unload function doesn't exist or returns an error.
  if started {
    if let Some(function) = unload_function {
      // rust doesn't allow null function pointers, so "unimplemented_unload" serves as a sentinel value that the
      // unload function is not implemented.
      if function != unimplemented_unload {
        //Safety: this is unsafe (even though rust doesn't think so) because we are calling
        //into the "unload" function pointer that the image itself set. r_efi doesn't mark
        //the unload function type as unsafe - so rust reports an "unused_unsafe" since it
        //doesn't know it's unsafe. We suppress the warning and mark it unsafe anyway as a
        //warning to the future.
        #[allow(unused_unsafe)]
        unsafe {
          let status = (function)(image_handle);
          if status != efi::Status::SUCCESS {
            Err(status)?;
          }
        }
      } else if !force_unload {
        Err(efi::Status::UNSUPPORTED)?;
      }
    } else if !force_unload {
      Err(efi::Status::UNSUPPORTED)?;
    }
  }
  let handles = PROTOCOL_DB.locate_handles(None).unwrap_or(Vec::new());

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
  let private_image_data = PRIVATE_IMAGE_DATA.lock().private_image_data.remove(&image_handle).unwrap();
  // remove the image and device path protocols from the image handle.
  let _ = PROTOCOL_DB.uninstall_protocol_interface(
    image_handle,
    efi::protocols::loaded_image::PROTOCOL_GUID,
    private_image_data.image_info_ptr,
  );

  let _ = PROTOCOL_DB.uninstall_protocol_interface(
    image_handle,
    efi::protocols::loaded_image_device_path::PROTOCOL_GUID,
    private_image_data.image_device_path_ptr,
  );

  Ok(())
}

extern "efiapi" fn unload_image(image_handle: efi::Handle) -> efi::Status {
  match core_unload_image(image_handle, false) {
    Ok(()) => efi::Status::SUCCESS,
    Err(err) => err,
  }
}

// Terminates a loaded EFI image and returns control to boot services.
// See EFI_BOOT_SERVICES::Exit() API definition in UEFI spec for usage details.
// * image_handle - the handle of the currently running image.
// * exit_status - the exit status for the image.
// * exit_data_size - the size of the exit_data buffer, if exit_data is not
//                    null.
// * exit_data - optional buffer of data provided by the caller.
extern "efiapi" fn exit(
  image_handle: efi::Handle,
  status: efi::Status,
  exit_data_size: usize,
  exit_data: *mut efi::Char16,
) -> efi::Status {
  // check the currently running image.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  if Some(image_handle) != private_data.current_running_image {
    return efi::Status::INVALID_PARAMETER;
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
  efi::Status::ACCESS_DENIED
}

/// Initializes image services for the DXE core.
pub fn init_image_support(hob_list: &HobList, system_table: &mut EfiSystemTable) {
  // initialize system table entry in private global.
  let mut private_data = PRIVATE_IMAGE_DATA.lock();
  private_data.system_table = system_table.as_ptr() as *mut efi::SystemTable;
  drop(private_data);

  // install the image protocol for the dxe_core.
  install_dxe_core_image(hob_list);

  //set up imaging services
  system_table.boot_services().load_image = load_image;
  system_table.boot_services().start_image = start_image;
  system_table.boot_services().unload_image = unload_image;
  system_table.boot_services().exit = exit;
}
