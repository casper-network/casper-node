use async_trait::async_trait;

#[async_trait]
pub trait NodeInterface: Send + Sync + 'static {
    async fn read(&self, db: Database, key: &[u8]) -> Result<Vec<u8>, Error>;
    async fn write(&self, db: Database, bytes: &[u8]) -> Result<(), Error>;
}

#[derive(Debug, Clone, Copy)]
pub enum Database {
    Transaction,
    FinalizedApprovals,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub struct DummyNodeInterface;

#[async_trait]
impl NodeInterface for DummyNodeInterface {
    async fn read(&self, _db: Database, _key: &[u8]) -> Result<Vec<u8>, Error> {
        todo!()
    }

    async fn write(&self, _db: Database, _bytes: &[u8]) -> Result<(), Error> {
        todo!()
    }
}
