pub fn roblox_variants(ch: char) -> Vec<char> {
    if ch == '름' { return vec!['름', '늠', '음']; }
    let Some((cho, jung, jong)) = decompose(ch) else { return vec![ch] };
    let mut out = vec![ch];
    if cho == 5 && matches!(jung, 2 | 6 | 7 | 12 | 17 | 20) {
        if let Some(c) = compose(11, jung, jong) { out.push(c); }
    } else if cho == 5 && matches!(jung, 0 | 1 | 8 | 11 | 13 | 18) {
        if let Some(c) = compose(2, jung, jong) { out.push(c); }
    } else if cho == 2 && matches!(jung, 6 | 12 | 17 | 20) {
        if let Some(c) = compose(11, jung, jong) { out.push(c); }
    } else if cho == 5 {
        if let Some(c) = compose(11, jung, jong) { out.push(c); }
    }
    out.sort_unstable(); out.dedup(); out
}

fn decompose(ch: char) -> Option<(u32,u32,u32)> {
    let code = ch as u32;
    if !(0xAC00..=0xD7A3).contains(&code) { return None; }
    let s = code - 0xAC00;
    Some((s / 588, (s % 588) / 28, s % 28))
}
fn compose(cho: u32, jung: u32, jong: u32) -> Option<char> {
    char::from_u32(0xAC00 + (cho * 21 + jung) * 28 + jong)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn special() { let v=roblox_variants('름'); assert!(v.contains(&'늠')&&v.contains(&'음')); }
    #[test] fn standard() { assert!(roblox_variants('력').contains(&'역')); assert!(roblox_variants('녀').contains(&'여')); }
}
