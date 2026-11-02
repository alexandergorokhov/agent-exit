//! Exported function names of a wasm module (the scan's method list).
//! Every read is bounds-checked and counts are capped (review 3 L5).

const MAX_EXPORTS: u32 = 10_000;

fn leb(b: &[u8], i: &mut usize) -> Option<u32> {
    let mut r: u64 = 0;
    for shift in (0..35).step_by(7) {
        let x = *b.get(*i)?;
        *i += 1;
        r |= ((x & 0x7f) as u64) << shift;
        if x & 0x80 == 0 {
            return u32::try_from(r).ok();
        }
    }
    None
}

pub fn exports(b: &[u8]) -> Result<Vec<String>, String> {
    if b.len() < 8 || &b[..4] != b"\0asm" {
        return Err("not a wasm module".into());
    }
    let mut i = 8;
    while i < b.len() {
        let id = b[i];
        i += 1;
        let size = leb(b, &mut i).ok_or("bad section size")? as usize;
        let end = i
            .checked_add(size)
            .filter(|e| *e <= b.len())
            .ok_or("section past the end")?;
        if id == 7 {
            let mut j = i;
            let n = leb(b, &mut j).ok_or("bad export count")?;
            if n > MAX_EXPORTS {
                return Err("too many exports".into());
            }
            let mut out = vec![];
            for _ in 0..n {
                let len = leb(b, &mut j).ok_or("bad name")? as usize;
                let name_end = j
                    .checked_add(len)
                    .filter(|e| *e <= end)
                    .ok_or("name past the section")?;
                let name = String::from_utf8_lossy(&b[j..name_end]).to_string();
                j = name_end;
                let kind = *b
                    .get(j)
                    .filter(|_| j < end)
                    .ok_or("kind past the section")?;
                j += 1;
                leb(b, &mut j).ok_or("bad index")?;
                if j > end {
                    return Err("export past the section".into());
                }
                if kind == 0 {
                    out.push(name);
                }
            }
            return Ok(out);
        }
        i = end;
    }
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exports_and_refuses_garbage() {
        // (module (func) (export "pay" (func 0)) (export "mem" (memory 0)))-like
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        let sec = [2u8, 3, b'p', b'a', b'y', 0, 0, 3, b'm', b'e', b'm', 2, 0];
        m.push(7);
        m.push(sec.len() as u8);
        m.extend_from_slice(&sec);
        assert_eq!(exports(&m).unwrap(), vec!["pay".to_string()]);
        assert!(exports(b"nope").is_err());
        let mut huge = b"\0asm\x01\0\0\0".to_vec();
        huge.extend_from_slice(&[7, 5, 0xff, 0xff, 0xff, 0xff, 0x0f]);
        assert!(exports(&huge).is_err());
        let mut short = m.clone();
        short.truncate(m.len() - 3);
        assert!(exports(&short).is_err());
    }
}
