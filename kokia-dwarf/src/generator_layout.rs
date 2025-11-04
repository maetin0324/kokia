//! Generator レイアウト解析（discriminant位置の特定）

use crate::Result;
use gimli::Reader;
use tracing::debug;

/// Discriminant情報
#[derive(Debug, Clone)]
pub struct DiscriminantLayout {
    /// Discriminantのオフセット（バイト）
    pub offset: u64,
    /// Discriminantのサイズ（バイト）
    pub size: u64,
}

/// Generatorレイアウトアナライザー
pub struct GeneratorLayoutAnalyzer<'a> {
    dwarf: &'a gimli::Dwarf<gimli::EndianSlice<'a, gimli::RunTimeEndian>>,
}

impl<'a> GeneratorLayoutAnalyzer<'a> {
    pub fn new(dwarf: &'a gimli::Dwarf<gimli::EndianSlice<'a, gimli::RunTimeEndian>>) -> Self {
        Self { dwarf }
    }

    /// Generator型のdiscriminant情報を取得
    ///
    /// # Arguments
    /// * `type_name` - Generator型の名前（関数名から推測）
    ///
    /// # Returns
    /// Discriminant情報、見つからない場合はNone
    pub fn get_discriminant_layout(&self, type_name: &str) -> Result<Option<DiscriminantLayout>> {
        debug!("get_discriminant_layout for type_name='{}'", type_name);
        // DWARFからgenerator enum型を検索
        let mut iter = self.dwarf.units();
        while let Some(header) = iter.next()? {
            let unit = self.dwarf.unit(header)?;

            if let Some(layout) = self.find_discriminant_in_unit(&unit, type_name)? {
                debug!("Found discriminant in DWARF: offset={}, size={}", layout.offset, layout.size);
                return Ok(Some(layout));
            }
        }

        // 見つからなかった場合、デフォルトの配置を返す
        // Rustのenum discriminantは通常、構造体の先頭（offset 0）にu32として配置される
        debug!("Discriminant not found in DWARF, using default (offset=0, size=4)");
        Ok(Some(DiscriminantLayout {
            offset: 0,
            size: 4,
        }))
    }

    /// ユニット内でgenerator型のdiscriminantを探す
    fn find_discriminant_in_unit<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        type_name: &str,
    ) -> Result<Option<DiscriminantLayout>> {
        let mut entries = unit.entries();
        let mut closure_types_found = 0;
        let mut structure_types_found = 0;
        let mut enumeration_types_found = 0;
        let mut names_extracted = 0;
        let mut sample_names: Vec<String> = Vec::new();

        while let Some((_, entry)) = entries.next_dfs()? {
            // enum型（DW_TAG_structure_type または DW_TAG_enumeration_type）を探す
            if entry.tag() == gimli::DW_TAG_structure_type {
                structure_types_found += 1;
            } else if entry.tag() == gimli::DW_TAG_enumeration_type {
                enumeration_types_found += 1;
            }

            if entry.tag() == gimli::DW_TAG_structure_type
                || entry.tag() == gimli::DW_TAG_enumeration_type
            {
                // 型名をチェック
                if let Some(name) = self.get_entry_name(entry)? {
                    names_extracted += 1;
                    if sample_names.len() < 10 {
                        sample_names.push(name.clone());
                    }

                    // generator型は "{closure_env", "{async_block_env", "{async_fn_env" という名前を持つ
                    if name.contains("{closure") || name.contains("{async_block") || name.contains("{async_fn") {
                        closure_types_found += 1;
                        if closure_types_found <= 10 {
                            debug!("Found closure type in DWARF: '{}'", name);
                        }

                        // マッチングロジック：
                        // type_name = "simple_async::main::{{closure}}"
                        // DWARFにある型:
                        //   - "{async_block_env#0}" (generator state machine)
                        //   - "{closure_env#0}<simple_async::main::{async_block#0}::{async_block_env#0}>" (closureラッパー)
                        //
                        // 優先順位:
                        // 1. "{async_block_env#0}" のような単独の型（generator state machine）
                        // 2. トップレベルでmodule pathが一致する型
                        let type_prefix = type_name.split("::{{").next().unwrap_or(type_name);

                        // パターン1: {async_block_env#0}, {async_fn_env#0} 単独（最優先）
                        if name == "{async_block_env#0}" || name == "{closure_env#0}" || name == "{async_fn_env#0}" {
                            debug!("Matched generator state machine: '{}' with '{}'",
                                name, type_name);
                            // discriminantフィールドを探す
                            return self.find_discriminant_field(unit, entry);
                        }

                        // パターン2: トップレベルでmodule pathが一致
                        let is_toplevel_match = {
                            if name.starts_with(type_prefix) {
                                true
                            } else if let Some(content) = name.strip_prefix("{closure_env#0}<")
                                        .or_else(|| name.strip_prefix("{async_block_env#0}<"))
                                        .or_else(|| name.strip_prefix("{async_fn_env#0}<")) {
                                content.starts_with(type_prefix)
                            } else {
                                false
                            }
                        };

                        if is_toplevel_match {
                            debug!("Matched closure wrapper (toplevel): '{}' with '{}'",
                                name, type_name);
                            // discriminantフィールドを探す
                            return self.find_discriminant_field(unit, entry);
                        }
                    }
                }
            }
        }

        if closure_types_found > 0 {
            eprintln!("DEBUG: Found {} closure types total (searched {} structures, {} enums), but none matched '{}'",
                closure_types_found, structure_types_found, enumeration_types_found, type_name);
        } else {
            eprintln!("DEBUG: No closure types found in this unit ({} structures, {} enums, {} names extracted)",
                structure_types_found, enumeration_types_found, names_extracted);
            if !sample_names.is_empty() {
                eprintln!("DEBUG: Sample type names: {:?}", &sample_names[..sample_names.len().min(5)]);
            }
        }

        Ok(None)
    }

    /// entry から名前を取得
    fn get_entry_name<R: Reader<Offset = usize>>(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<String>> {
        match entry.attr_value(gimli::DW_AT_name)? {
            Some(gimli::AttributeValue::String(s)) => {
                Ok(Some(s.to_string_lossy()?.into_owned()))
            }
            Some(gimli::AttributeValue::DebugStrRef(offset)) => {
                // DebugStrRefを処理する
                if let Ok(s) = self.dwarf.string(offset) {
                    Ok(Some(s.to_string_lossy().into_owned()))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// enum型内でdiscriminantフィールドを探す
    fn find_discriminant_field<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        parent: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<DiscriminantLayout>> {
        let mut tree = unit.entries_tree(Some(parent.offset()))?;
        let root = tree.root()?;

        // まずDW_TAG_variant_partを探す（これがRustのgenerator discriminantの正しい方法）
        let mut children = root.children();
        while let Some(child) = children.next()? {
            let entry = child.entry();

            if entry.tag() == gimli::DW_TAG_variant_part {
                eprintln!("DEBUG: Found DW_TAG_variant_part");
                // DW_AT_discr属性でdiscriminant memberを特定
                if let Some(gimli::AttributeValue::UnitRef(discr_offset)) = entry.attr_value(gimli::DW_AT_discr)? {
                    eprintln!("DEBUG: Found DW_AT_discr pointing to offset {:?}", discr_offset);
                    // Discriminant memberエントリを取得
                    let mut entries = unit.entries_at_offset(discr_offset)?;
                    if let Some((_, discr_entry)) = entries.next_dfs()? {
                        let name = self.get_entry_name(discr_entry)?.unwrap_or_else(|| "<unnamed>".to_string());
                        let offset = self.get_member_offset(discr_entry)?;
                        let size = self.get_member_size(unit, discr_entry)?;

                        eprintln!("DEBUG: Discriminant field '{}' at offset={:?}, size={:?}", name, offset, size);

                        return Ok(Some(DiscriminantLayout {
                            offset: offset.unwrap_or(0),
                            size: size.unwrap_or(4),
                        }));
                    }
                }
            }
        }

        // フォールバック: 古い方法（"__0", "discriminant", 最初のフィールド）
        eprintln!("DEBUG: No DW_TAG_variant_part found, trying fallback method");
        // rootを再度取得
        let mut tree2 = unit.entries_tree(Some(parent.offset()))?;
        let root2 = tree2.root()?;
        let mut children2 = root2.children();
        while let Some(child) = children2.next()? {
            let entry = child.entry();

            if entry.tag() == gimli::DW_TAG_member {
                if let Some(name) = self.get_entry_name(entry)? {
                    if name == "__0" || name == "discriminant" || name == "__state" {
                        let offset = self.get_member_offset(entry)?;
                        let size = self.get_member_size(unit, entry)?;

                        eprintln!("DEBUG: Found discriminant field by name '{}': offset={:?}, size={:?}", name, offset, size);

                        return Ok(Some(DiscriminantLayout {
                            offset: offset.unwrap_or(0),
                            size: size.unwrap_or(4),
                        }));
                    }
                }
            }
        }

        eprintln!("DEBUG: No discriminant field found");
        Ok(None)
    }

    /// メンバーのオフセットを取得
    fn get_member_offset<R: Reader<Offset = usize>>(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<u64>> {
        match entry.attr_value(gimli::DW_AT_data_member_location)? {
            Some(gimli::AttributeValue::Udata(offset)) => Ok(Some(offset)),
            Some(gimli::AttributeValue::Data1(offset)) => Ok(Some(offset as u64)),
            Some(gimli::AttributeValue::Data2(offset)) => Ok(Some(offset as u64)),
            Some(gimli::AttributeValue::Data4(offset)) => Ok(Some(offset as u64)),
            Some(gimli::AttributeValue::Data8(offset)) => Ok(Some(offset)),
            _ => Ok(None),
        }
    }

    /// メンバーのサイズを取得（型情報から）
    fn get_member_size<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<u64>> {
        // DW_AT_type から型参照を取得
        let type_offset = match entry.attr_value(gimli::DW_AT_type)? {
            Some(gimli::AttributeValue::UnitRef(offset)) => offset,
            _ => return Ok(None),
        };

        // 型DIEを取得
        let mut entries = unit.entries_at_offset(type_offset)?;
        if let Some((_, type_entry)) = entries.next_dfs()? {
            // DW_AT_byte_size からサイズを取得
            match type_entry.attr_value(gimli::DW_AT_byte_size)? {
                Some(gimli::AttributeValue::Udata(size)) => return Ok(Some(size)),
                Some(gimli::AttributeValue::Data1(size)) => return Ok(Some(size as u64)),
                Some(gimli::AttributeValue::Data2(size)) => return Ok(Some(size as u64)),
                Some(gimli::AttributeValue::Data4(size)) => return Ok(Some(size as u64)),
                Some(gimli::AttributeValue::Data8(size)) => return Ok(Some(size)),
                _ => {}
            }
        }

        Ok(None)
    }

    /// Generator型の variant 情報を取得
    ///
    /// # Arguments
    /// * `type_name` - Generator型の名前
    /// * `discriminant_value` - Discriminant値
    ///
    /// # Returns
    /// Variant情報（フィールドリスト等）
    pub fn get_variant_info(
        &self,
        type_name: &str,
        discriminant_value: u64,
    ) -> Result<Option<VariantInfo>> {
        // DWARFからgenerator enum型を検索
        let mut iter = self.dwarf.units();
        while let Some(header) = iter.next()? {
            let unit = self.dwarf.unit(header)?;

            if let Some(variant) = self.find_variant_in_unit(&unit, type_name, discriminant_value)? {
                return Ok(Some(variant));
            }
        }

        Ok(None)
    }

    /// ユニット内でvariantを探す
    fn find_variant_in_unit<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        type_name: &str,
        discriminant_value: u64,
    ) -> Result<Option<VariantInfo>> {
        let mut entries = unit.entries();
        let mut candidates = Vec::new();
        let mut closure_count = 0;

        while let Some((_, entry)) = entries.next_dfs()? {
            // enum型を探す
            if entry.tag() == gimli::DW_TAG_structure_type
                || entry.tag() == gimli::DW_TAG_enumeration_type
            {
                if let Some(name) = self.get_entry_name(entry)? {
                    if name.contains("{closure") || name.contains("{async_block") || name.contains("{async_fn") {
                        closure_count += 1;

                        // ラッパー型（<を含む）をスキップ - これらは実際のジェネレーター状態マシンではない
                        if name.contains('<') {
                            eprintln!("DEBUG: find_variant_in_unit: Skipping wrapper type '{}'", name);
                            continue;
                        }

                        // 非ラッパー型が見つかった
                        eprintln!("DEBUG: find_variant_in_unit: Found non-wrapper closure type '{}'", name);

                        // すべての候補を記録（ラッパーでないもののみ）
                        if name == "{async_block_env#0}" || name == "{closure_env#0}" || name == "{async_fn_env#0}" {
                            eprintln!("DEBUG: find_variant_in_unit: Found candidate '{}' at offset {:?}", name, entry.offset());
                            candidates.push((name.clone(), entry.offset()));
                        }

                        // マッチングロジック（get_discriminant_layoutと同じ）
                        let type_prefix = type_name.split("::{{").next().unwrap_or(type_name);
                        eprintln!("DEBUG: find_variant_in_unit: Checking if '{}' matches type_prefix '{}'", name, type_prefix);

                        // パターン1: {async_block_env#0}, {async_fn_env#0} 単独（最優先）
                        if name == "{async_block_env#0}" || name == "{closure_env#0}" || name == "{async_fn_env#0}" {
                            eprintln!("DEBUG: find_variant_in_unit: Pattern 1 exact match '{}'", name);
                            return self.extract_variant_info(unit, entry, discriminant_value);
                        }

                        // パターン2: トップレベルでmodule pathが一致（ラッパーではない型のみ）
                        let is_toplevel_match = name.starts_with(type_prefix);

                        if is_toplevel_match {
                            // variant情報を取得
                            eprintln!("DEBUG: find_variant_in_unit: Pattern 2 match: '{}' starts with '{}'", name, type_prefix);
                            return self.extract_variant_info(unit, entry, discriminant_value);
                        }
                    }
                }
            }
        }

        // すべての候補を試す
        eprintln!("DEBUG: find_variant_in_unit: Found {} closure types, {} candidates for '{}'", closure_count, candidates.len(), type_name);
        for (name, offset) in &candidates {
            eprintln!("DEBUG: Candidate: '{}' at offset {:?}", name, offset);
        }

        // 最初の候補を試す
        if !candidates.is_empty() {
            let (name, offset) = &candidates[0];
            eprintln!("DEBUG: Trying first candidate '{}' at offset {:?}", name, offset);

            // オフセットからエントリを取得
            let mut entries = unit.entries_at_offset(*offset)?;
            if let Some((_, entry)) = entries.next_dfs()? {
                return self.extract_variant_info(unit, entry, discriminant_value);
            }
        }

        Ok(None)
    }

    /// enum型からvariant情報を抽出
    fn extract_variant_info<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        parent: &gimli::DebuggingInformationEntry<R>,
        discriminant_value: u64,
    ) -> Result<Option<VariantInfo>> {
        eprintln!("DEBUG: extract_variant_info called for discriminant={}", discriminant_value);
        let mut tree = unit.entries_tree(Some(parent.offset()))?;
        let root = tree.root()?;

        // 子要素（variant）を走査
        let mut children = root.children();
        let mut variant_count = 0;
        while let Some(child) = children.next()? {
            let entry = child.entry();
            eprintln!("DEBUG: Examining child tag: {:?}", entry.tag());

            // DW_TAG_variant_part を探す（variantのコンテナ）
            if entry.tag() == gimli::DW_TAG_variant_part {
                eprintln!("DEBUG: Found variant_part, examining its children");
                // variant_partの子要素（実際のvariant）を走査
                let mut variant_children = child.children();
                while let Some(variant_child) = variant_children.next()? {
                    let variant_entry = variant_child.entry();

                    if variant_entry.tag() == gimli::DW_TAG_variant {
                        variant_count += 1;
                        eprintln!("DEBUG: Found variant #{}", variant_count);

                        // discriminant値が一致するか確認
                        if let Some(discr_val) = self.get_variant_discriminant(variant_entry)? {
                            eprintln!("DEBUG: Variant has discriminant={}, looking for={}", discr_val, discriminant_value);
                            if discr_val == discriminant_value {
                                // variant名とフィールドを抽出
                                let name = self.get_entry_name(variant_entry)?
                                    .unwrap_or_else(|| format!("Variant{}", discriminant_value));
                                eprintln!("DEBUG: Found matching variant: {}", name);
                                let fields = self.extract_variant_fields(unit, variant_child)?;
                                eprintln!("DEBUG: Extracted {} fields from variant", fields.len());

                                return Ok(Some(VariantInfo { name, fields }));
                            }
                        } else {
                            eprintln!("DEBUG: Variant has no discriminant value");
                        }
                    }
                }
            }
            // 直接のDW_TAG_variantも処理（フォールバック）
            else if entry.tag() == gimli::DW_TAG_variant {
                variant_count += 1;
                eprintln!("DEBUG: Found variant #{}", variant_count);
                // discriminant値が一致するか確認
                if let Some(discr_val) = self.get_variant_discriminant(entry)? {
                    eprintln!("DEBUG: Variant has discriminant={}, looking for={}", discr_val, discriminant_value);
                    if discr_val == discriminant_value {
                        // variant名とフィールドを抽出
                        let name = self.get_entry_name(entry)?
                            .unwrap_or_else(|| format!("Variant{}", discriminant_value));
                        eprintln!("DEBUG: Found matching variant: {}", name);
                        let fields = self.extract_variant_fields(unit, child)?;
                        eprintln!("DEBUG: Extracted {} fields from variant", fields.len());

                        return Ok(Some(VariantInfo { name, fields }));
                    }
                } else {
                    eprintln!("DEBUG: Variant has no discriminant value");
                }
            }
        }

        // variantが見つからなかった場合、デフォルト情報を返す
        eprintln!("DEBUG: No matching variant found, returning default empty variant");
        Ok(Some(VariantInfo {
            name: format!("State{}", discriminant_value),
            fields: Vec::new(),
        }))
    }

    /// variantのdiscriminant値を取得
    fn get_variant_discriminant<R: Reader<Offset = usize>>(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<u64>> {
        match entry.attr_value(gimli::DW_AT_discr_value)? {
            Some(gimli::AttributeValue::Udata(val)) => Ok(Some(val)),
            Some(gimli::AttributeValue::Data1(val)) => Ok(Some(val as u64)),
            Some(gimli::AttributeValue::Data2(val)) => Ok(Some(val as u64)),
            Some(gimli::AttributeValue::Data4(val)) => Ok(Some(val as u64)),
            Some(gimli::AttributeValue::Data8(val)) => Ok(Some(val)),
            _ => Ok(None),
        }
    }

    /// variantのフィールドを抽出
    fn extract_variant_fields<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        variant_node: gimli::EntriesTreeNode<R>,
    ) -> Result<Vec<FieldInfo>> {
        let mut fields = Vec::new();
        let mut children = variant_node.children();

        while let Some(child) = children.next()? {
            let entry = child.entry();

            if entry.tag() == gimli::DW_TAG_member {
                // このメンバーが参照している型を取得
                if let Some(gimli::AttributeValue::UnitRef(type_offset)) = entry.attr_value(gimli::DW_AT_type)? {
                    // 型DIEを取得
                    let mut type_entries = unit.entries_at_offset(type_offset)?;
                    if let Some((_, type_entry)) = type_entries.next_dfs()? {
                        // 型が構造体の場合、その中のフィールドを抽出
                        if type_entry.tag() == gimli::DW_TAG_structure_type {
                            eprintln!("DEBUG: Found structure type in variant, extracting fields from it");
                            // 構造体の中のフィールドを再帰的に取得
                            let struct_fields = self.extract_struct_fields(unit, type_entry)?;
                            fields.extend(struct_fields);
                        } else {
                            // 構造体ではない場合は、メンバー自体をフィールドとして扱う
                            if let Some(field) = self.extract_field_info(unit, entry)? {
                                fields.push(field);
                            }
                        }
                    }
                }
            }
        }

        Ok(fields)
    }

    /// 構造体のフィールドを抽出
    fn extract_struct_fields<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        struct_entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Vec<FieldInfo>> {
        eprintln!("DEBUG: extract_struct_fields called for entry at offset {:?}, tag={:?}",
                 struct_entry.offset(), struct_entry.tag());
        let mut fields = Vec::new();

        // 構造体名を表示
        if let Some(name) = self.get_entry_name(struct_entry)? {
            eprintln!("DEBUG: extract_struct_fields: struct name='{}'", name);
        }

        // 構造体のサブツリーを作成
        let mut tree = unit.entries_tree(Some(struct_entry.offset()))?;
        let root = tree.root()?;
        eprintln!("DEBUG: extract_struct_fields: created tree, root offset={:?}, tag={:?}",
                 root.entry().offset(), root.entry().tag());

        // 構造体の子要素（フィールド）を走査
        let mut children = root.children();
        let mut child_count = 0;
        while let Some(child) = children.next()? {
            child_count += 1;
            let entry = child.entry();
            eprintln!("DEBUG: Examining struct child #{}: tag={:?}, offset={:?}",
                     child_count, entry.tag(), entry.offset());

            if entry.tag() == gimli::DW_TAG_member {
                eprintln!("DEBUG: Found DW_TAG_member");
                if let Some(field) = self.extract_field_info(unit, entry)? {
                    eprintln!("DEBUG: Extracted field: name={}, offset={}, size={}",
                             field.name, field.offset, field.size);
                    fields.push(field);
                }
            }
        }

        eprintln!("DEBUG: extract_struct_fields found {} children, {} fields", child_count, fields.len());
        Ok(fields)
    }

    /// フィールド情報を抽出
    fn extract_field_info<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<FieldInfo>> {
        let name = self.get_entry_name(entry)?
            .unwrap_or_else(|| "<unnamed>".to_string());
        let offset = self.get_member_offset(entry)?.unwrap_or(0);
        let size = self.get_member_size(unit, entry)?.unwrap_or(0);
        let type_name = self.get_member_type_name(unit, entry)?;

        Ok(Some(FieldInfo {
            name,
            offset,
            size,
            type_name,
        }))
    }

    /// メンバーの型名を取得
    fn get_member_type_name<R: Reader<Offset = usize>>(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<String>> {
        // DW_AT_type から型参照を取得
        let type_offset = match entry.attr_value(gimli::DW_AT_type)? {
            Some(gimli::AttributeValue::UnitRef(offset)) => offset,
            _ => return Ok(None),
        };

        // 型DIEを取得
        let mut entries = unit.entries_at_offset(type_offset)?;
        if let Some((_, type_entry)) = entries.next_dfs()? {
            return self.get_entry_name(type_entry);
        }

        Ok(None)
    }
}

/// Variant情報
#[derive(Debug, Clone)]
pub struct VariantInfo {
    /// Variant名
    pub name: String,
    /// フィールド情報のリスト
    pub fields: Vec<FieldInfo>,
}

/// フィールド情報
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// フィールド名
    pub name: String,
    /// オフセット
    pub offset: u64,
    /// サイズ
    pub size: u64,
    /// 型名
    pub type_name: Option<String>,
}
