#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalChannelKind {
    DigitalInput,
    DigitalOutput,
    AnalogInput,
    AnalogOutput,
}

impl LogicalChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DigitalInput => "di",
            Self::DigitalOutput => "do",
            Self::AnalogInput => "ai",
            Self::AnalogOutput => "ao",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalChannel {
    pub kind: LogicalChannelKind,
    pub id: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum IecAddressParseError {
    #[error("invalid IEC address {input:?}: must start with '%'")]
    MissingPercent { input: String },

    #[error("invalid IEC address {input:?}: expected %IXn.m, %QXn.m, %IWn, or %QWn")]
    InvalidFormat { input: String },

    #[error("invalid IEC address {input:?}: invalid numeric value")]
    InvalidNumber { input: String },

    #[error("invalid IEC address {input:?}: bit index must be 0..=7, got {bit}")]
    BitOutOfRange { input: String, bit: u8 },

    #[error("invalid IEC address {input:?}: computed bit index overflows u16")]
    IdOverflow { input: String },
}

/// Parse a minimal subset of IEC 61131-3 address syntax for tool-chain alias mapping.
///
/// Supported:
/// - `%IXn.m` -> DI id = n*8+m
/// - `%QXn.m` -> DO id = n*8+m
/// - `%IWn`   -> AI id = n
/// - `%QWn`   -> AO id = n
///
/// Notes:
/// - Leading/trailing whitespace is allowed.
/// - The `%I/%Q` and `X/W` parts are case-insensitive.
pub fn parse_iec_address(input: &str) -> Result<LogicalChannel, IecAddressParseError> {
    let raw = input.trim();
    if !raw.starts_with('%') {
        return Err(IecAddressParseError::MissingPercent {
            input: raw.to_string(),
        });
    }
    let rest = &raw[1..];
    let mut chars = rest.chars();
    let Some(iq) = chars.next() else {
        return Err(IecAddressParseError::InvalidFormat {
            input: raw.to_string(),
        });
    };
    let Some(xw) = chars.next() else {
        return Err(IecAddressParseError::InvalidFormat {
            input: raw.to_string(),
        });
    };

    let iq = iq.to_ascii_uppercase();
    let xw = xw.to_ascii_uppercase();
    let tail = chars.as_str();

    match (iq, xw) {
        ('I', 'X') | ('Q', 'X') => {
            let (n_str, m_str) = tail.split_once('.').ok_or_else(|| {
                IecAddressParseError::InvalidFormat {
                    input: raw.to_string(),
                }
            })?;
            let n: u32 = n_str.parse().map_err(|_| IecAddressParseError::InvalidNumber {
                input: raw.to_string(),
            })?;
            let m_u32: u32 = m_str.parse().map_err(|_| IecAddressParseError::InvalidNumber {
                input: raw.to_string(),
            })?;
            let m: u8 = m_u32
                .try_into()
                .map_err(|_| IecAddressParseError::InvalidNumber {
                    input: raw.to_string(),
                })?;
            if m > 7 {
                return Err(IecAddressParseError::BitOutOfRange {
                    input: raw.to_string(),
                    bit: m,
                });
            }
            let id_u32 = n.saturating_mul(8).saturating_add(m as u32);
            let id: u16 = id_u32
                .try_into()
                .map_err(|_| IecAddressParseError::IdOverflow {
                    input: raw.to_string(),
                })?;
            let kind = if iq == 'I' {
                LogicalChannelKind::DigitalInput
            } else {
                LogicalChannelKind::DigitalOutput
            };
            Ok(LogicalChannel { kind, id })
        }
        ('I', 'W') | ('Q', 'W') => {
            if tail.is_empty() {
                return Err(IecAddressParseError::InvalidFormat {
                    input: raw.to_string(),
                });
            }
            let n: u16 = tail.parse().map_err(|_| IecAddressParseError::InvalidNumber {
                input: raw.to_string(),
            })?;
            let kind = if iq == 'I' {
                LogicalChannelKind::AnalogInput
            } else {
                LogicalChannelKind::AnalogOutput
            };
            Ok(LogicalChannel { kind, id: n })
        }
        _ => Err(IecAddressParseError::InvalidFormat {
            input: raw.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ix_bits_into_di_ids() {
        assert_eq!(
            parse_iec_address("%IX0.0").unwrap(),
            LogicalChannel {
                kind: LogicalChannelKind::DigitalInput,
                id: 0
            }
        );
        assert_eq!(parse_iec_address("%IX0.7").unwrap().id, 7);
        assert_eq!(parse_iec_address("%IX1.0").unwrap().id, 8);
    }

    #[test]
    fn parses_qx_bits_into_do_ids_case_insensitive_and_trimmed() {
        assert_eq!(
            parse_iec_address("  %qx2.3 ").unwrap(),
            LogicalChannel {
                kind: LogicalChannelKind::DigitalOutput,
                id: 19
            }
        );
    }

    #[test]
    fn parses_iw_qw_words_into_ai_ao_ids() {
        assert_eq!(
            parse_iec_address("%IW5").unwrap(),
            LogicalChannel {
                kind: LogicalChannelKind::AnalogInput,
                id: 5
            }
        );
        assert_eq!(
            parse_iec_address("%QW12").unwrap(),
            LogicalChannel {
                kind: LogicalChannelKind::AnalogOutput,
                id: 12
            }
        );
    }

    #[test]
    fn rejects_invalid_formats_and_bit_range() {
        assert!(matches!(
            parse_iec_address("IX0.0").unwrap_err(),
            IecAddressParseError::MissingPercent { .. }
        ));
        assert!(matches!(
            parse_iec_address("%IX0").unwrap_err(),
            IecAddressParseError::InvalidFormat { .. }
        ));
        assert!(matches!(
            parse_iec_address("%IX0.8").unwrap_err(),
            IecAddressParseError::BitOutOfRange { .. }
        ));
        assert!(matches!(
            parse_iec_address("%MW0").unwrap_err(),
            IecAddressParseError::InvalidFormat { .. }
        ));
    }
}

