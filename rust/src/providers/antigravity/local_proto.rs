#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ParsedUsage {
    pub system_prompt: u64,
    pub new_input: u64,
    pub cache_read: u64,
    pub output: u64,
    pub reasoning: u64,
    pub response_id: Option<String>,
    pub bot_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ParsedTurn {
    pub usage: Option<ParsedUsage>,
    pub timestamp_ms: Option<i64>,
    pub model: Option<String>,
    pub label: Option<String>,
    pub step_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepMetadata {
    pub step_uuid: Option<String>,
    pub bot_id: Option<String>,
    pub timestamp_ms: Option<i64>,
}

#[derive(Clone, Copy)]
enum FieldValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}
#[derive(Clone, Copy)]
struct Field<'a> {
    number: u32,
    value: FieldValue<'a>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn read_varint(&mut self) -> Option<u64> {
        let mut result = 0_u64;
        for index in 0..10 {
            let byte = *self.bytes.get(self.offset)?;
            self.offset += 1;
            if index == 9 && byte > 1 {
                return None;
            }
            result |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Some(result);
            }
        }
        None
    }
    fn next_field(&mut self) -> Option<Field<'a>> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let tag = self.read_varint()?;
        let number = u32::try_from(tag >> 3).ok()?;
        if number == 0 || number > 536_870_911 {
            return None;
        }
        match tag & 7 {
            0 => Some(Field {
                number,
                value: FieldValue::Varint(self.read_varint()?),
            }),
            1 | 5 => {
                let width = if tag & 7 == 1 { 8 } else { 4 };
                let end = self.offset.checked_add(width)?;
                let data = self.bytes.get(self.offset..end)?;
                self.offset = end;
                Some(Field {
                    number,
                    value: FieldValue::Bytes(data),
                })
            }
            2 => {
                let count = usize::try_from(self.read_varint()?).ok()?;
                let end = self.offset.checked_add(count)?;
                let data = self.bytes.get(self.offset..end)?;
                self.offset = end;
                Some(Field {
                    number,
                    value: FieldValue::Bytes(data),
                })
            }
            _ => None,
        }
    }
}

fn fields(bytes: &[u8], mut visit: impl FnMut(Field<'_>) -> Option<()>) -> Option<()> {
    let mut reader = Reader::new(bytes);
    while reader.offset < bytes.len() {
        visit(reader.next_field()?)?;
    }
    Some(())
}
fn message(field: Field<'_>) -> Option<&[u8]> {
    match field.value {
        FieldValue::Bytes(v) => Some(v),
        _ => None,
    }
}
fn integer(field: Field<'_>) -> Option<u64> {
    match field.value {
        FieldValue::Varint(v) => Some(v),
        _ => None,
    }
}
fn text(field: Field<'_>) -> Option<Option<String>> {
    let value = std::str::from_utf8(message(field)?).ok()?.trim();
    Some((!value.is_empty()).then(|| value.to_string()))
}

fn identity_text(field: Field<'_>) -> Option<Option<String>> {
    let value = std::str::from_utf8(message(field)?).ok()?;
    Some((!value.trim().is_empty()).then(|| value.to_string()))
}

fn auxiliary_text(field: Field<'_>) -> Result<Option<String>, ()> {
    let value = std::str::from_utf8(message(field).ok_or(())?).map_err(|_| ())?;
    Ok((!value.trim().is_empty()).then(|| value.to_string()))
}

#[derive(Debug, Default)]
struct AuxiliaryIdentifier {
    value: Option<String>,
    valid: bool,
}

impl AuxiliaryIdentifier {
    fn new() -> Self {
        Self {
            value: None,
            valid: true,
        }
    }

    fn read(&mut self, field: Field<'_>) {
        if !self.valid {
            return;
        }
        match auxiliary_text(field) {
            Ok(value) => self.value = value,
            Err(()) => {
                self.value = None;
                self.valid = false;
            }
        }
    }

    fn invalidate(&mut self) {
        self.value = None;
        self.valid = false;
    }
}

fn timestamp_millis(seconds: Option<u64>, nanos: u64) -> Option<Option<i64>> {
    let Some(seconds) = seconds else {
        return Some(None);
    };
    if seconds == 0 || seconds > 253_402_300_799 || nanos > 999_999_999 {
        return None;
    }
    let seconds = i64::try_from(seconds).ok()?;
    let nanos = i64::try_from(nanos).ok()?;
    Some(Some(
        seconds.checked_mul(1000)?.checked_add(nanos / 1_000_000)?,
    ))
}

pub(super) fn parse_turn(root: &[u8]) -> Option<ParsedTurn> {
    let mut turn = ParsedTurn::default();
    let mut seconds = None;
    let mut nanos = 0_u64;
    let mut found_chat = false;
    fields(root, |field| {
        if field.number != 1 {
            if field.number == 4 {
                turn.step_uuid = identity_text(field)?;
            }
            return Some(());
        }
        found_chat = true;
        parse_chat(message(field)?, &mut turn, &mut seconds, &mut nanos)
    })?;
    if !found_chat {
        return None;
    }
    turn.timestamp_ms = timestamp_millis(seconds, nanos)?;
    Some(turn)
}

pub(super) fn parse_step_metadata(root: &[u8]) -> Option<StepMetadata> {
    let mut step_uuid = None;
    let mut bot_identifier = AuxiliaryIdentifier::new();
    let mut seconds = None;
    let mut nanos = 0_u64;
    let mut timestamp_is_valid = true;
    fields(root, |field| {
        match field.number {
            1 => {
                let Some(timestamp) = message(field) else {
                    timestamp_is_valid = false;
                    return Some(());
                };
                if parse_timestamp_field(timestamp, &mut seconds, &mut nanos).is_none() {
                    timestamp_is_valid = false;
                }
            }
            9 => {
                let Some(envelope) = message(field) else {
                    bot_identifier.invalidate();
                    return Some(());
                };
                if fields(envelope, |subfield| {
                    if subfield.number == 7 {
                        bot_identifier.read(subfield);
                    }
                    Some(())
                })
                .is_none()
                {
                    bot_identifier.invalidate();
                }
            }
            12 => step_uuid = identity_text(field)?,
            _ => {}
        }
        Some(())
    })?;
    let timestamp_ms = if timestamp_is_valid {
        timestamp_millis(seconds, nanos)?
    } else {
        None
    };
    Some(StepMetadata {
        step_uuid,
        bot_id: bot_identifier.value,
        timestamp_ms,
    })
}

fn parse_chat(
    bytes: &[u8],
    turn: &mut ParsedTurn,
    seconds: &mut Option<u64>,
    nanos: &mut u64,
) -> Option<()> {
    fields(bytes, |field| {
        match field.number {
            4 => {
                let mut usage = turn.usage.take().unwrap_or_default();
                parse_usage(message(field)?, &mut usage)?;
                turn.usage = Some(usage);
            }
            9 => parse_generation(message(field)?, seconds, nanos)?,
            19 => turn.model = text(field)?,
            21 => turn.label = text(field)?,
            _ => {}
        }
        Some(())
    })
}
fn parse_usage(bytes: &[u8], usage: &mut ParsedUsage) -> Option<()> {
    let mut bot_identifier = AuxiliaryIdentifier::new();
    fields(bytes, |field| {
        match field.number {
            1 => usage.system_prompt = integer(field)?,
            2 => usage.new_input = integer(field)?,
            5 => usage.cache_read = integer(field)?,
            7 => bot_identifier.read(field),
            9 => usage.output = integer(field)?,
            10 => usage.reasoning = integer(field)?,
            11 => usage.response_id = text(field)?,
            _ => {}
        }
        Some(())
    })?;
    usage.bot_id = bot_identifier.value;
    Some(())
}
fn parse_generation(bytes: &[u8], seconds: &mut Option<u64>, nanos: &mut u64) -> Option<()> {
    fields(bytes, |field| {
        if field.number != 4 {
            return Some(());
        }
        parse_timestamp_field(message(field)?, seconds, nanos)
    })
}

fn parse_timestamp_field(bytes: &[u8], seconds: &mut Option<u64>, nanos: &mut u64) -> Option<()> {
    fields(bytes, |stamp| {
        match stamp.number {
            1 => {
                let value = integer(stamp)?;
                if value == 0 || value > 253_402_300_799 {
                    return None;
                }
                *seconds = Some(value);
            }
            2 => {
                let value = integer(stamp)?;
                if value > 999_999_999 {
                    return None;
                }
                *nanos = value;
            }
            _ => {}
        }
        Some(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }
    fn field_varint(number: u64, value: u64) -> Vec<u8> {
        let mut b = varint(number << 3);
        b.extend(varint(value));
        b
    }
    fn field_bytes(number: u64, value: &[u8]) -> Vec<u8> {
        let mut b = varint((number << 3) | 2);
        b.extend(varint(value.len() as u64));
        b.extend(value);
        b
    }
    #[test]
    fn decodes_generation_usage_and_timestamp() {
        let mut usage = Vec::new();
        usage.extend(field_varint(1, 10));
        usage.extend(field_varint(2, 20));
        usage.extend(field_varint(5, 30));
        usage.extend(field_varint(9, 40));
        usage.extend(field_varint(10, 50));
        usage.extend(field_bytes(11, b"response-1"));
        let mut stamp = Vec::new();
        stamp.extend(field_varint(1, 1_787_572_800));
        stamp.extend(field_varint(2, 123_000_000));
        let generation = field_bytes(4, &stamp);
        let mut chat = Vec::new();
        chat.extend(field_bytes(4, &usage));
        chat.extend(field_bytes(9, &generation));
        chat.extend(field_bytes(19, b"test-model-antigravity-a"));
        chat.extend(field_bytes(21, b"label-a"));
        let root = field_bytes(1, &chat);
        let turn = parse_turn(&root).unwrap();
        let usage = turn.usage.unwrap();
        assert_eq!(usage.system_prompt, 10);
        assert_eq!(usage.new_input, 20);
        assert_eq!(usage.cache_read, 30);
        assert_eq!(usage.output, 40);
        assert_eq!(usage.reasoning, 50);
        assert_eq!(usage.response_id.as_deref(), Some("response-1"));
        assert_eq!(usage.bot_id, None);
        assert_eq!(turn.timestamp_ms, Some(1_787_572_800_123));
    }
    #[test]
    fn rejects_malformed_varint() {
        assert!(parse_turn(&[0x0a, 0x80]).is_none());
    }

    #[test]
    fn decodes_new_root_envelope_step_uuid_and_step_metadata() {
        let step_uuid = "step-uuid-1";
        let mut usage = Vec::new();
        usage.extend(field_varint(1, 10));
        usage.extend(field_varint(2, 20));
        usage.extend(field_varint(9, 40));
        let chat = field_bytes(4, &usage);
        let mut root = field_bytes(2, &[1, 2]);
        root.extend(field_bytes(4, step_uuid.as_bytes()));
        root.extend(field_bytes(1, &chat));

        let turn = parse_turn(&root).unwrap();
        assert_eq!(turn.step_uuid.as_deref(), Some(step_uuid));
        assert_eq!(turn.timestamp_ms, None);

        let timestamp = field_varint(1, 1_787_572_800);
        let metadata = field_bytes(
            1,
            &timestamp
                .into_iter()
                .chain(field_varint(2, 123_000_000))
                .collect::<Vec<_>>(),
        );
        let mut metadata = metadata;
        metadata.extend(field_bytes(12, step_uuid.as_bytes()));
        assert_eq!(
            parse_step_metadata(&metadata),
            Some(StepMetadata {
                step_uuid: Some(step_uuid.to_string()),
                bot_id: None,
                timestamp_ms: Some(1_787_572_800_123),
            })
        );
    }

    #[test]
    fn invalid_step_timestamp_is_evidence_without_a_timestamp() {
        let mut metadata = field_bytes(1, &field_varint(2, 1_000_000_000));
        metadata.extend(field_bytes(12, b"step-uuid-1"));

        assert_eq!(
            parse_step_metadata(&metadata),
            Some(StepMetadata {
                step_uuid: Some("step-uuid-1".to_string()),
                bot_id: None,
                timestamp_ms: None,
            })
        );
    }

    #[test]
    fn malformed_optional_bot_id_does_not_discard_usage() {
        let mut usage = field_varint(2, 20);
        usage.extend(field_bytes(7, &[0xff]));
        usage.extend(field_varint(9, 40));
        let root = field_bytes(1, &field_bytes(4, &usage));

        let turn = parse_turn(&root).expect("malformed optional ID must not discard turn");
        assert_eq!(turn.usage.unwrap().bot_id, None);
    }

    #[test]
    fn malformed_step_bot_id_disables_exact_identity_but_keeps_metadata() {
        let mut envelope = field_bytes(7, &[0xff]);
        envelope.extend(field_bytes(7, b"later-valid-id"));
        let mut metadata = field_bytes(9, &envelope);
        metadata.extend(field_bytes(12, b"step-uuid-1"));

        assert_eq!(
            parse_step_metadata(&metadata),
            Some(StepMetadata {
                step_uuid: Some("step-uuid-1".to_string()),
                bot_id: None,
                timestamp_ms: None,
            })
        );
    }
}
