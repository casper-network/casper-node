use casper_binary_port::{RawBytesSpec, RecordId};
use casper_storage::{DbRawBytesSpec, DbTableId, UnknownDbTableId};
use std::convert::TryFrom;

pub(crate) fn db_table_id_from_record_id(
    record_id: RecordId,
) -> Result<DbTableId, UnknownDbTableId> {
    DbTableId::try_from(record_id as u16)
}

pub(crate) fn raw_bytes_from_db_raw_bytes(db_raw_bytes_spec: DbRawBytesSpec) -> RawBytesSpec {
    RawBytesSpec::new(
        &db_raw_bytes_spec.raw_bytes(),
        db_raw_bytes_spec.is_legacy(),
    )
}
