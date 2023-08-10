use core::{fmt::Debug, mem, ops::Deref, slice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
  NotEnoughMemory,
  ElementAlreadyInserted,
  ElementsNeedToBeSorted,
}

pub trait SortedSliceKey {
  type Key: Ord;
  fn ordering_key(&self) -> &Self::Key;
}

pub struct SortedSlice<'a, T> {
  slice: &'a mut [T],
  item_count: usize,
}

impl<'a, T> SortedSlice<'a, T>
where
  T: Clone + Copy + SortedSliceKey + Sized,
{
  pub fn new(slice: &'a mut [u8]) -> SortedSlice<'a, T> {
    Self {
      slice: unsafe {
        slice::from_raw_parts_mut::<'a, T>(slice.as_mut() as *mut [u8] as *mut T, slice.len() / mem::size_of::<T>())
      },
      item_count: 0,
    }
  }

  pub fn add(&mut self, element: T) -> Result<usize, Error> {
    if self.capacity() == self.len() {
      return Err(Error::NotEnoughMemory);
    }
    let Err(idx) = self.search(element) else {
      return Err(Error::ElementAlreadyInserted);
    };

    self.slice.copy_within(idx..self.len(), idx + 1);
    self.slice[idx] = element;
    self.item_count += 1;
    Ok(idx)
  }

  pub fn add_contiguous_slice(&mut self, elements: &[T]) -> Result<usize, Error> {
    if elements.is_empty() {
      return Ok(0);
    }

    if self.len() + elements.len() > self.capacity() {
      return Err(Error::NotEnoughMemory);
    }

    if !elements.is_sorted_by_key(|e| e.ordering_key()) {
      return Err(Error::ElementsNeedToBeSorted);
    }

    let mut e = elements.windows(2);
    while let Some([a, b]) = e.next() {
      if a.ordering_key() == b.ordering_key() {
        return Err(Error::ElementAlreadyInserted);
      }
    }

    let Err(idx) = self.search(elements[0]) else {
      return Err(Error::ElementAlreadyInserted);
    };

    if let Some(next) = self.get(idx) {
      let last = elements[elements.len() - 1];
      match last.ordering_key().cmp(next.ordering_key()) {
        core::cmp::Ordering::Equal => return Err(Error::ElementAlreadyInserted),
        core::cmp::Ordering::Greater => return Err(Error::ElementsNeedToBeSorted),
        _ => (),
      }
    }

    self.slice.copy_within(idx..self.len(), idx + elements.len());
    self.slice[idx..idx + elements.len()].copy_from_slice(elements);
    self.item_count += elements.len();
    Ok(idx)
  }

  pub fn remove(&mut self, element: T) -> Result<usize, ()> {
    let Ok(idx) = self.search(element) else {
      return Err(());
    };
    self.remove_at_idx(idx);
    Ok(idx)
  }

  pub fn remove_at_idx(&mut self, idx: usize) -> Option<T> {
    if idx >= self.item_count {
      return None;
    }
    let item = self.slice[idx];
    self.slice.copy_within(idx + 1..self.len(), idx);
    self.item_count -= 1;
    Some(item)
  }

  pub fn search(&self, element: T) -> Result<usize, usize> {
    let target = element.ordering_key();
    self.binary_search_by_key(&target, |e| e.ordering_key())
  }

  pub fn search_with_key(&self, key: &T::Key) -> Result<&T, &T> {
    self.binary_search_by_key(&key, |e| e.ordering_key()).map(|idx| &self[idx]).map_err(|idx| &self[idx])
  }

  pub fn search_with_key_mut(&mut self, key: &T::Key) -> Result<&mut T, &mut T> {
    let index = self.binary_search_by_key(&key, |e| e.ordering_key());
    match index {
      Ok(idx) => Ok(&mut self[idx]),
      Err(idx) => Err(&mut self[idx]),
    }
  }

  pub fn search_idx_with_key(&mut self, key: &T::Key) -> Result<usize, usize> {
    self.binary_search_by_key(&key, |e| e.ordering_key())
  }

  pub fn capacity(&self) -> usize {
    self.slice.len()
  }
}

impl<T> core::ops::Deref for SortedSlice<'_, T> {
  type Target = [T];

  fn deref(&self) -> &Self::Target {
    &self.slice[..self.item_count]
  }
}

// TODO Maybe adding manually the interesting function and add a way to mutate element that validate that is still sorted after.
impl<T> core::ops::DerefMut for SortedSlice<'_, T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.slice[..self.item_count]
  }
}

impl<'a, T> IntoIterator for &'a SortedSlice<'a, T> {
  type Item = &'a T;
  type IntoIter = slice::Iter<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter()
  }
}

impl<'a, T> IntoIterator for &'a mut SortedSlice<'a, T> {
  type Item = &'a mut T;
  type IntoIter = slice::IterMut<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter_mut()
  }
}

impl<T> core::fmt::Debug for SortedSlice<'_, T>
where
  T: Debug,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("MemoryBlockSlice").field("block_count", &self.item_count).field("slice", &self.deref()).finish()
  }
}

impl<T> SortedSliceKey for T
where
  T: Ord,
{
  type Key = Self;
  fn ordering_key(&self) -> &T {
    &self
  }
}

#[cfg(test)]
mod tests {

  use super::*;

  #[test]
  fn test_init_state_of_new_sorted_slice() {
    const MEM_SIZE: usize = 4096;
    let mut mem = [0; MEM_SIZE];
    let mem_ptr = mem.as_ptr();
    let ss = SortedSlice::<'_, u32>::new(&mut mem);

    assert_eq!(0, ss.item_count);
    assert_eq!(mem_ptr, ss.slice.as_ptr() as *const u8);
    assert_eq!(MEM_SIZE / mem::size_of::<u32>(), ss.slice.len());
    assert_eq!(MEM_SIZE / mem::size_of::<u32>(), ss.capacity());
    assert_eq!(0, ss.len(), "The deref impl should only return the used part of the slice.");
  }

  #[test]
  fn test_add_in_sorted_slice() {
    let mut mem = [0; 10 * mem::size_of::<usize>()];
    let mut ss = SortedSlice::<'_, usize>::new(&mut mem);

    for e in [1, 4, 3, 2, 5, 8, 0, 6, 7] {
      ss.add(e).unwrap();
    }
    for i in 0..9 {
      assert_eq!(i, ss[i], "The add operation should keep the slice sorted.");
    }

    assert_eq!(Err(Error::ElementAlreadyInserted), ss.add(0), "The slide should not allow duplicates.");
    assert_eq!(Ok(9), ss.add(9));
    assert_eq!(Err(Error::NotEnoughMemory), ss.add(10), "Need to error if there is not enough space to add element.");
  }

  #[test]
  fn test_add_contiguous_slice_in_sorted_array() {
    let mut mem = [0; 10 * mem::size_of::<usize>()];
    let mut ss = SortedSlice::<'_, usize>::new(&mut mem);

    assert_eq!(Err(Error::ElementsNeedToBeSorted), ss.add_contiguous_slice(&[2, 1]));
    assert_eq!(0, ss.len());

    assert_eq!(Err(Error::NotEnoughMemory), ss.add_contiguous_slice(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
    assert_eq!(0, ss.len());

    ss.add(0).unwrap();
    ss.add(1).unwrap();
    ss.add(8).unwrap();
    ss.add(9).unwrap();

    assert_eq!(Err(Error::ElementAlreadyInserted), ss.add_contiguous_slice(&[5, 6, 7, 8]));
    assert_eq!(4, ss.len());
    assert_eq!(Err(Error::ElementAlreadyInserted), ss.add_contiguous_slice(&[1, 5, 6, 7]));
    assert_eq!(4, ss.len());
    assert_eq!(Err(Error::ElementsNeedToBeSorted), ss.add_contiguous_slice(&[5, 6, 7, 9]));
    assert_eq!(4, ss.len());

    assert_eq!(Ok(2), ss.add_contiguous_slice(&[2, 3, 4, 5, 6]));
    assert_eq!(9, ss.len());
    assert_eq!(Ok(7), ss.add_contiguous_slice(&[7]));
    assert_eq!(10, ss.len());

    assert_eq!(Err(Error::NotEnoughMemory), ss.add_contiguous_slice(&[11]));
  }

  #[test]
  fn test_remove_in_sorted_array() {
    let mut mem = [0; 10 * mem::size_of::<usize>()];
    let mut ss = SortedSlice::new(&mut mem);

    ss.add_contiguous_slice(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();

    assert_eq!(Ok(5), ss.remove(5));
    assert_eq!(Err(()), ss.remove(5));

    let mut len = ss.len();
    for e in [3, 2, 4, 9, 0, 1, 8, 7, 6] {
      ss.remove(e).unwrap();
      len -= 1;
      assert_eq!(len, ss.len());
    }

    ss.add_contiguous_slice(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
    for i in 0..ss.len() {
      assert_eq!(Some(i), ss.remove_at_idx(0));
    }
  }

  #[test]
  fn test_iter_sorted_slice() {
    let mut mem = [0; 10 * mem::size_of::<usize>()];
    let mut ss = SortedSlice::new(&mut mem);

    let items = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    ss.add_contiguous_slice(&items).unwrap();
    assert_eq!(items.iter().collect::<Vec<_>>(), ss.iter().collect::<Vec<_>>());
  }
}
