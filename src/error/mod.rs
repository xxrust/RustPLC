use std::error::Error;
use std::fmt;

const DEFAULT_FILE: &str = "<input>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl SourceLocation {
    pub fn new(file: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }

    pub fn from_line(line: usize) -> Self {
        Self::new(DEFAULT_FILE, line, 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlcError {
    ParseError {
        location: SourceLocation,
        message: String,
        reason: Option<String>,
    },
    SemanticError {
        location: SourceLocation,
        message: String,
        reason: Option<String>,
    },
    UndefinedReference {
        location: SourceLocation,
        reference_type: String,
        name: String,
        reason: Option<String>,
    },
    TypeMismatch {
        location: SourceLocation,
        expected: String,
        found: String,
        context: Option<String>,
        reason: Option<String>,
    },
    DuplicateDefinition {
        location: SourceLocation,
        definition_type: String,
        name: String,
        reason: Option<String>,
    },
}

impl PlcError {
    pub fn parse(line: usize, message: impl Into<String>) -> Self {
        Self::ParseError {
            location: SourceLocation::from_line(line),
            message: message.into(),
            reason: None,
        }
    }

    pub fn parse_at(
        file: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self::ParseError {
            location: SourceLocation::new(file, line, column),
            message: message.into(),
            reason: None,
        }
    }

    pub fn parse_with_reason(
        line: usize,
        message: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::ParseError {
            location: SourceLocation::from_line(line),
            message: message.into(),
            reason: Some(reason.into()),
        }
    }

    pub fn semantic(line: usize, message: impl Into<String>) -> Self {
        Self::SemanticError {
            location: SourceLocation::from_line(line),
            message: message.into(),
            reason: None,
        }
    }

    pub fn semantic_with_reason(
        line: usize,
        message: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::SemanticError {
            location: SourceLocation::from_line(line),
            message: message.into(),
            reason: Some(reason.into()),
        }
    }

    pub fn undefined_reference(
        line: usize,
        reference_type: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::UndefinedReference {
            location: SourceLocation::from_line(line),
            reference_type: reference_type.into(),
            name: name.into(),
            reason: None,
        }
    }

    pub fn undefined_reference_with_reason(
        line: usize,
        reference_type: impl Into<String>,
        name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::UndefinedReference {
            location: SourceLocation::from_line(line),
            reference_type: reference_type.into(),
            name: name.into(),
            reason: Some(reason.into()),
        }
    }

    pub fn type_mismatch(
        line: usize,
        expected: impl Into<String>,
        found: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::TypeMismatch {
            location: SourceLocation::from_line(line),
            expected: expected.into(),
            found: found.into(),
            context: Some(context.into()),
            reason: None,
        }
    }

    pub fn type_mismatch_with_reason(
        line: usize,
        expected: impl Into<String>,
        found: impl Into<String>,
        context: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::TypeMismatch {
            location: SourceLocation::from_line(line),
            expected: expected.into(),
            found: found.into(),
            context: Some(context.into()),
            reason: Some(reason.into()),
        }
    }

    pub fn duplicate_definition(
        line: usize,
        definition_type: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::DuplicateDefinition {
            location: SourceLocation::from_line(line),
            definition_type: definition_type.into(),
            name: name.into(),
            reason: None,
        }
    }

    pub fn duplicate_definition_with_reason(
        line: usize,
        definition_type: impl Into<String>,
        name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::DuplicateDefinition {
            location: SourceLocation::from_line(line),
            definition_type: definition_type.into(),
            name: name.into(),
            reason: Some(reason.into()),
        }
    }

    pub fn device_library_parse_error(path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::ParseError {
            location: SourceLocation::new(path, 0, 0),
            message: format!("设备库 TOML 解析失败: {}", detail.into()),
            reason: Some("请检查 TOML 文件格式".into()),
        }
    }

    pub fn device_library_io_error(path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::ParseError {
            location: SourceLocation::new(path, 0, 0),
            message: format!("设备库文件读取失败: {}", detail.into()),
            reason: None,
        }
    }

    pub fn device_library_invalid_port_ref(
        port_state: impl Into<String>,
        instance: impl Into<String>,
    ) -> Self {
        let ps = port_state.into();
        let inst = instance.into();
        Self::SemanticError {
            location: SourceLocation::from_line(0),
            message: format!("设备库约束端口引用格式错误: {ps} (设备实例: {inst})"),
            reason: Some("端口引用格式应为 port.state".into()),
        }
    }

    pub fn line(&self) -> usize {
        self.location().line
    }

    pub fn column(&self) -> usize {
        self.location().column
    }

    pub fn location(&self) -> &SourceLocation {
        match self {
            Self::ParseError { location, .. }
            | Self::SemanticError { location, .. }
            | Self::UndefinedReference { location, .. }
            | Self::TypeMismatch { location, .. }
            | Self::DuplicateDefinition { location, .. } => location,
        }
    }
}

impl fmt::Display for PlcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError {
                location,
                message,
                reason,
            } => write_error_block(f, "parse", "语法错误", location, message, reason.as_deref()),
            Self::SemanticError {
                location,
                message,
                reason,
            } => write_error_block(
                f,
                "semantic",
                "语义错误",
                location,
                message,
                reason.as_deref(),
            ),
            Self::UndefinedReference {
                location,
                reference_type,
                name,
                reason,
            } => {
                let detail = format!("未定义{reference_type} {name}");
                write_error_block(
                    f,
                    "undefined_reference",
                    "未定义引用",
                    location,
                    &detail,
                    reason.as_deref(),
                )
            }
            Self::TypeMismatch {
                location,
                expected,
                found,
                context,
                reason,
            } => {
                let detail = if let Some(context) = context {
                    format!("{context} 类型不匹配，期望 {expected}，实际 {found}")
                } else {
                    format!("类型不匹配，期望 {expected}，实际 {found}")
                };
                write_error_block(
                    f,
                    "type_mismatch",
                    "类型不匹配",
                    location,
                    &detail,
                    reason.as_deref(),
                )
            }
            Self::DuplicateDefinition {
                location,
                definition_type,
                name,
                reason,
            } => {
                let detail = format!("重复定义{definition_type} {name}");
                write_error_block(
                    f,
                    "duplicate_definition",
                    "重复定义",
                    location,
                    &detail,
                    reason.as_deref(),
                )
            }
        }
    }
}

impl Error for PlcError {}

fn write_error_block(
    f: &mut fmt::Formatter<'_>,
    code: &str,
    title: &str,
    location: &SourceLocation,
    detail: &str,
    reason: Option<&str>,
) -> fmt::Result {
    write!(
        f,
        "ERROR [{code}] {title}\n  位置: {}:{}:{}\n  原因: {detail}",
        location.file, location.line, location.column,
    )?;

    if let Some(reason) = reason {
        write!(f, "\n  建议: {reason}")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PlcError;

    #[test]
    fn format_parse_error_contains_line_and_chinese_message() {
        let err = PlcError::parse_at("main.plc", 12, 7, "缺少 [tasks] 段");
        let rendered = err.to_string();

        assert!(rendered.contains("ERROR [parse]"));
        assert!(rendered.contains("位置: main.plc:12:7"));
        assert!(rendered.contains("原因: 缺少 [tasks] 段"));
    }

    #[test]
    fn format_undefined_reference_contains_missing_device_name() {
        let err = PlcError::undefined_reference_with_reason(
            9,
            "设备",
            "Y9",
            "请先在 [topology] 段定义该设备",
        );
        let rendered = err.to_string();

        assert!(rendered.contains("ERROR [undefined_reference]"));
        assert!(rendered.contains("未定义设备 Y9"));
        assert!(rendered.contains("建议: 请先在 [topology] 段定义该设备"));
    }
}
