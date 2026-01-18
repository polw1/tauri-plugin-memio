//! Schema metadata for struct field access.

#[derive(Debug, Clone, Copy)]
pub enum MemioScalarType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl MemioScalarType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MemioFieldType {
    Scalar(MemioScalarType),
    Array { elem: MemioScalarType, len: usize },
}

#[derive(Debug, Clone, Copy)]
pub struct MemioField {
    pub name: &'static str,
    pub offset: usize,
    pub ty: MemioFieldType,
}

pub trait MemioSchema {
    fn schema() -> &'static [MemioField];
}

/// Generates JSON representation of schema fields.
pub fn schema_json<T: MemioSchema>() -> String {
    let fields = T::schema();
    let mut out = String::from("{\"fields\":[");
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":\"");
        push_json_string(&mut out, field.name);
        out.push_str("\",\"offset\":");
        out.push_str(&field.offset.to_string());
        match field.ty {
            MemioFieldType::Scalar(ty) => {
                out.push_str(",\"type\":\"");
                out.push_str(ty.as_str());
                out.push_str("\"}");
            }
            MemioFieldType::Array { elem, len } => {
                out.push_str(",\"type\":\"array\",\"elem\":\"");
                out.push_str(elem.as_str());
                out.push_str("\",\"len\":");
                out.push_str(&len.to_string());
                out.push('}');
            }
        }
    }
    out.push_str("]}");
    out
}

fn push_json_string(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}
