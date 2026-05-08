fn chapter_number_from_title(chapter: &str) -> Option<i64> {
    let digits = chapter
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if !digits.is_empty() {
        return digits.parse::<i64>().ok();
    }
    let start = chapter.find('第')?;
    let rest = &chapter[start + '第'.len_utf8()..];
    let end = rest.find('章').unwrap_or(rest.len());
    let raw = rest[..end].trim();
    parse_chinese_number(raw)
}

fn parse_chinese_number(raw: &str) -> Option<i64> {
    let digit = |ch: char| match ch {
        '零' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    if raw.is_empty() {
        return None;
    }
    if raw == "十" {
        return Some(10);
    }
    if let Some(idx) = raw.find('十') {
        let left = raw[..idx].chars().next().and_then(digit).unwrap_or(1);
        let right = raw[idx + '十'.len_utf8()..]
            .chars()
            .next()
            .and_then(digit)
            .unwrap_or(0);
        return Some((left * 10 + right) as i64);
    }
    let mut value = 0i64;
    for ch in raw.chars() {
        value = value * 10 + i64::from(digit(ch)?);
    }
    Some(value)
}
