use capnp::traits::IntoInternalStructReader;

use crate::capnp::global_state_capnp::global_state_entry;

impl<'a> global_state_entry::Reader<'a> {
    /// Get the tag identifying which variant this entry is.
    pub fn get_tag(&self) -> u16 {
        self.reborrow()
            .into_internal_struct_reader()
            .get_data_field::<u16>(0)
    }
}
