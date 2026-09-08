#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexScanReadReceipt {
    pub metadata_reads: u32,
    pub history_reads: u32,
}
