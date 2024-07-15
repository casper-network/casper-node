use crate::EntryPoint;

/// Check if a fallback selector exists in a 2D array of entry points.
pub const fn fallback_selector_count(selectors: &[&[EntryPoint]]) -> usize {
    let mut i = 0;
    let mut count = 0;
    while i < selectors.len() {
        let mut j = 0;
        while j < selectors[i].len() {
            if selectors[i][j].selector == 0 && selectors[i][j].flags & 0x0000_0002 != 0 {
                count += 1;
            }
            j += 1;
        }
        i += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn foo() {}

    // Test case 1: Empty selectors
    #[allow(dead_code)]
    const EMPTY_SELECTORS: &[&[EntryPoint]] = &[&[]];
    const _: () = {
        assert!(fallback_selector_count(EMPTY_SELECTORS) == 0);
    };

    // Test case 2: No fallback selector
    #[allow(dead_code)]
    const INVALID_SELECTORS: &[&[EntryPoint]] = &[
        &[
            EntryPoint {
                selector: 1,
                fptr: foo,
                flags: 0,
            },
            EntryPoint {
                selector: 2,
                fptr: foo,
                flags: 0,
            },
        ],
        &[
            EntryPoint {
                selector: 3,
                fptr: foo,
                flags: 0,
            },
            EntryPoint {
                selector: 4,
                fptr: foo,
                flags: 0,
            },
        ],
    ];
    const _: () = {
        assert!(fallback_selector_count(INVALID_SELECTORS) == 0);
    };

    // Test case 3: Fallback selector exists
    #[allow(dead_code)]
    const VALID_SELECTORS: &[&[EntryPoint]] = &[
        &[
            EntryPoint {
                selector: 1,
                fptr: foo,
                flags: 0,
            },
            EntryPoint {
                selector: 0,
                fptr: foo,
                flags: 0x02,
            },
        ],
        &[
            EntryPoint {
                selector: 3,
                fptr: foo,
                flags: 0,
            },
            EntryPoint {
                selector: 4,
                fptr: foo,
                flags: 0,
            },
        ],
        &[],
        &[EntryPoint {
            selector: 0,
            fptr: foo,
            flags: 0x02,
        }],
    ];
    const _: () = {
        assert!(fallback_selector_count(VALID_SELECTORS) == 2);
    };
}
