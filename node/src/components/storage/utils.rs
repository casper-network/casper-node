use casper_binary_port::RecordId;
use casper_storage::{DbTableId, UnknownDbTableId};
use std::convert::TryFrom;

pub(crate) fn db_table_id_from_record_id(
    record_id: RecordId,
) -> Result<DbTableId, UnknownDbTableId> {
    DbTableId::try_from(record_id as u16)
}
