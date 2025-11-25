//! DWARF型情報の抽出
//!
//! DWARF DIEから型情報（構造体フィールド、列挙型variant等）を抽出します。

use crate::Result;
use gimli::Reader;

/// 型情報
#[derive(Debug, Clone)]
pub enum TypeInfo {
    /// 基本型
    Primitive {
        name: String,
        size: u64,
    },
    /// ポインタ型
    Pointer {
        pointee_type: Option<Box<TypeInfo>>,
        size: u64,
    },
    /// 参照型
    Reference {
        referent_type: Option<Box<TypeInfo>>,
        size: u64,
    },
    /// 配列型
    Array {
        element_type: Option<Box<TypeInfo>>,
        length: Option<u64>,
    },
    /// 構造体型
    Struct {
        name: String,
        size: u64,
        fields: Vec<FieldInfo>,
    },
    /// 列挙型
    Enum {
        name: String,
        size: u64,
        variants: Vec<VariantInfo>,
    },
    /// Union型
    Union {
        name: String,
        size: u64,
        members: Vec<FieldInfo>,
    },
    /// 不明な型
    Unknown,
}

/// フィールド情報
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// フィールド名
    pub name: String,
    /// オフセット（バイト）
    pub offset: u64,
    /// サイズ（バイト）
    pub size: u64,
    /// 型情報
    pub type_info: Option<Box<TypeInfo>>,
}

/// Variant情報
#[derive(Debug, Clone)]
pub struct VariantInfo {
    /// Variant名
    pub name: String,
    /// Discriminant値
    pub discriminant: Option<u64>,
    /// フィールド
    pub fields: Vec<FieldInfo>,
}

/// 型情報抽出器
pub struct TypeInfoExtractor<'a, R: Reader> {
    #[allow(dead_code)]
    dwarf: &'a gimli::Dwarf<R>,
}

impl<'a, R: Reader<Offset = usize>> TypeInfoExtractor<'a, R> {
    /// 新しい型情報抽出器を作成する
    pub fn new(dwarf: &'a gimli::Dwarf<R>) -> Self {
        Self { dwarf }
    }

    /// 型DIEから型情報を抽出する
    pub fn extract_type_info(
        &self,
        unit: &gimli::Unit<R>,
        type_offset: gimli::UnitOffset<R::Offset>,
    ) -> Result<TypeInfo> {
        let mut entries = unit.entries_at_offset(type_offset)?;

        if let Some((_, entry)) = entries.next_dfs()? {
            self.extract_from_entry(unit, entry)
        } else {
            Ok(TypeInfo::Unknown)
        }
    }

    /// DIEエントリから型情報を抽出する
    fn extract_from_entry(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        match entry.tag() {
            gimli::DW_TAG_base_type => self.extract_base_type(entry),
            gimli::DW_TAG_pointer_type => self.extract_pointer_type(unit, entry),
            gimli::DW_TAG_reference_type => self.extract_reference_type(unit, entry),
            gimli::DW_TAG_array_type => self.extract_array_type(unit, entry),
            gimli::DW_TAG_structure_type => self.extract_struct_type(unit, entry),
            gimli::DW_TAG_enumeration_type => self.extract_enum_type(unit, entry),
            gimli::DW_TAG_union_type => self.extract_union_type(unit, entry),
            _ => Ok(TypeInfo::Unknown),
        }
    }

    /// 基本型を抽出する
    fn extract_base_type(&self, entry: &gimli::DebuggingInformationEntry<R>) -> Result<TypeInfo> {
        let name = self.get_name(entry).unwrap_or_else(|| "<unknown>".to_string());
        let size = self.get_byte_size(entry).unwrap_or(0);

        Ok(TypeInfo::Primitive { name, size })
    }

    /// ポインタ型を抽出する
    fn extract_pointer_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let size = self.get_byte_size(entry).unwrap_or(8); // 64bit = 8 bytes

        // 参照先の型を取得
        let pointee_type = if let Some(type_offset) = self.get_type(entry) {
            match self.extract_type_info(unit, type_offset) {
                Ok(info) => Some(Box::new(info)),
                Err(_) => None,
            }
        } else {
            None
        };

        Ok(TypeInfo::Pointer {
            pointee_type,
            size,
        })
    }

    /// 参照型を抽出する
    fn extract_reference_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let size = self.get_byte_size(entry).unwrap_or(8);

        let referent_type = if let Some(type_offset) = self.get_type(entry) {
            match self.extract_type_info(unit, type_offset) {
                Ok(info) => Some(Box::new(info)),
                Err(_) => None,
            }
        } else {
            None
        };

        Ok(TypeInfo::Reference {
            referent_type,
            size,
        })
    }

    /// 配列型を抽出する
    fn extract_array_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let element_type = if let Some(type_offset) = self.get_type(entry) {
            match self.extract_type_info(unit, type_offset) {
                Ok(info) => Some(Box::new(info)),
                Err(_) => None,
            }
        } else {
            None
        };

        // 配列長を取得（子DIEから）
        // TODO: DW_TAG_subrange_typeから長さを取得
        let length = None;

        Ok(TypeInfo::Array {
            element_type,
            length,
        })
    }

    /// 構造体型を抽出する
    fn extract_struct_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let name = self.get_name(entry).unwrap_or_else(|| "<anonymous>".to_string());
        let size = self.get_byte_size(entry).unwrap_or(0);

        // フィールドを列挙
        let fields = self.extract_fields(unit, entry)?;

        Ok(TypeInfo::Struct { name, size, fields })
    }

    /// 列挙型を抽出する
    fn extract_enum_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let name = self.get_name(entry).unwrap_or_else(|| "<anonymous>".to_string());
        let size = self.get_byte_size(entry).unwrap_or(0);

        // Variantを列挙
        let variants = self.extract_variants(unit, entry)?;

        Ok(TypeInfo::Enum {
            name,
            size,
            variants,
        })
    }

    /// Union型を抽出する
    fn extract_union_type(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<TypeInfo> {
        let name = self.get_name(entry).unwrap_or_else(|| "<anonymous>".to_string());
        let size = self.get_byte_size(entry).unwrap_or(0);

        // メンバを列挙
        let members = self.extract_fields(unit, entry)?;

        Ok(TypeInfo::Union {
            name,
            size,
            members,
        })
    }

    /// フィールドを抽出する
    fn extract_fields(
        &self,
        unit: &gimli::Unit<R>,
        parent_entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Vec<FieldInfo>> {
        let mut fields = Vec::new();
        let mut tree = unit.entries_tree(Some(parent_entry.offset()))?;
        let root = tree.root()?;

        let mut children = root.children();
        while let Some(child) = children.next()? {
            let entry = child.entry();

            if entry.tag() == gimli::DW_TAG_member {
                if let Some(field) = self.extract_field(unit, entry)? {
                    fields.push(field);
                }
            }
        }

        Ok(fields)
    }

    /// フィールド情報を抽出する
    fn extract_field(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<FieldInfo>> {
        let name = self.get_name(entry).unwrap_or_else(|| "<unnamed>".to_string());
        let offset = self.get_data_member_location(entry).unwrap_or(0);

        // 型情報を取得
        let (type_info, size) = if let Some(type_offset) = self.get_type(entry) {
            match self.extract_type_info(unit, type_offset) {
                Ok(info) => {
                    let size = self.get_type_size(&info);
                    (Some(Box::new(info)), size)
                }
                Err(_) => (None, 0),
            }
        } else {
            (None, 0)
        };

        Ok(Some(FieldInfo {
            name,
            offset,
            size,
            type_info,
        }))
    }

    /// Variantを抽出する（簡易実装）
    fn extract_variants(
        &self,
        _unit: &gimli::Unit<R>,
        _parent_entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Vec<VariantInfo>> {
        // TODO: 列挙型のvariantを抽出する完全な実装
        Ok(Vec::new())
    }

    /// 名前を取得する
    fn get_name(&self, entry: &gimli::DebuggingInformationEntry<R>) -> Option<String> {
        let attr = entry.attr_value(gimli::DW_AT_name).ok()??;
        match attr {
            gimli::AttributeValue::String(s) => s.to_string_lossy().ok().map(|s| s.into_owned()),
            _ => None,
        }
    }

    /// バイトサイズを取得する
    fn get_byte_size(&self, entry: &gimli::DebuggingInformationEntry<R>) -> Option<u64> {
        let attr = entry.attr_value(gimli::DW_AT_byte_size).ok()??;
        match attr {
            gimli::AttributeValue::Udata(size) => Some(size),
            _ => None,
        }
    }

    /// 型参照を取得する
    fn get_type(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Option<gimli::UnitOffset<R::Offset>> {
        let attr = entry.attr_value(gimli::DW_AT_type).ok()??;
        match attr {
            gimli::AttributeValue::UnitRef(offset) => Some(offset),
            _ => None,
        }
    }

    /// データメンバのロケーション（オフセット）を取得する
    fn get_data_member_location(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Option<u64> {
        let attr = entry.attr_value(gimli::DW_AT_data_member_location).ok()??;
        match attr {
            gimli::AttributeValue::Udata(offset) => Some(offset),
            _ => None,
        }
    }

    /// 型情報からサイズを取得する
    fn get_type_size(&self, type_info: &TypeInfo) -> u64 {
        match type_info {
            TypeInfo::Primitive { size, .. } => *size,
            TypeInfo::Pointer { size, .. } => *size,
            TypeInfo::Reference { size, .. } => *size,
            TypeInfo::Struct { size, .. } => *size,
            TypeInfo::Enum { size, .. } => *size,
            TypeInfo::Union { size, .. } => *size,
            _ => 0,
        }
    }
}
