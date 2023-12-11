use borsh::BorshSerialize;

#[derive(Copy, Clone)]
pub struct Export {
    pub name: &'static str,
    pub fptr: fn(&[u8]) -> (),
}

impl std::fmt::Debug for Export {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Export").field("name", &self.name).finish()
    }
}

pub static mut LAZY_EXPORTS: [Option<Export>; 1024] = [None; 1024];

pub fn register_export(export: Export) {
    for l in unsafe { LAZY_EXPORTS.iter_mut() } {
        if l.is_none() {
            *l = Some(export);
            return;
        }
    }
    assert!(false, "no more space");
}

pub fn list_exports() -> Vec<&'static Export> {
    let mut v = Vec::new();
    for export_name in unsafe { LAZY_EXPORTS.iter() } {
        if let Some(export_name) = export_name {
            v.push(export_name);
        }
    }
    v
}

pub fn dispatch<Args: BorshSerialize>(name: &str, args: Args) {
    for export in list_exports() {
        if export.name == name {
            let data = borsh::to_vec(&args).unwrap();
            (export.fptr)(&data);
            return;
        }
    }
    panic!("not found");
}
