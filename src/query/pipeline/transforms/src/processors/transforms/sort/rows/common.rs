// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use common_exception::Result;
use common_expression::types::nullable::NullableColumn;
use common_expression::types::string::StringColumn;
use common_expression::types::string::StringColumnBuilder;
use common_expression::types::DataType;
use common_expression::BlockEntry;
use common_expression::Column;
use common_expression::ColumnBuilder;
use common_expression::DataSchemaRef;
use common_expression::RowConverter as CommonRowConverter;
use common_expression::Scalar;
use common_expression::SortColumnDescription;
use common_expression::SortField;
use common_expression::Value;
use jsonb::convert_to_comparable;

use super::RowConverter;
use super::Rows;

pub type CommonRows = StringColumn;

impl Rows for StringColumn {
    type Item<'a> = &'a [u8];

    fn len(&self) -> usize {
        self.len()
    }

    fn row(&self, index: usize) -> Self::Item<'_> {
        unsafe { self.index_unchecked(index) }
    }

    fn to_column(&self) -> Column {
        Column::String(self.clone())
    }

    fn from_column(col: Column, _: &[SortColumnDescription]) -> Option<Self> {
        col.as_string().cloned()
    }
}

impl RowConverter<StringColumn> for CommonRowConverter {
    fn create(
        sort_columns_descriptions: &[SortColumnDescription],
        output_schema: DataSchemaRef,
    ) -> Result<Self> {
        let sort_fields = sort_columns_descriptions
            .iter()
            .map(|d| {
                let data_type = output_schema.field(d.offset).data_type();
                SortField::new_with_options(data_type.clone(), d.asc, d.nulls_first)
            })
            .collect::<Vec<_>>();
        CommonRowConverter::new(sort_fields)
    }

    fn convert(&mut self, columns: &[BlockEntry], num_rows: usize) -> Result<StringColumn> {
        let columns = columns
            .iter()
            .map(|entry| match &entry.value {
                Value::Scalar(s) => match s {
                    Scalar::Variant(val) => {
                        // convert variant value to comparable format.
                        let mut buf = Vec::new();
                        convert_to_comparable(val, &mut buf);
                        let s = Scalar::Variant(buf);
                        ColumnBuilder::repeat(&s.as_ref(), num_rows, &entry.data_type).build()
                    }
                    _ => ColumnBuilder::repeat(&s.as_ref(), num_rows, &entry.data_type).build(),
                },
                Value::Column(c) => {
                    let data_type = c.data_type();
                    match data_type.remove_nullable() {
                        DataType::Variant => {
                            // convert variant value to comparable format.
                            let (_, validity) = c.validity();
                            let col = c.remove_nullable();
                            let col = col.as_variant().unwrap();
                            let mut builder =
                                StringColumnBuilder::with_capacity(col.len(), col.data().len());
                            for (i, val) in col.iter().enumerate() {
                                if let Some(validity) = validity {
                                    if unsafe { !validity.get_bit_unchecked(i) } {
                                        builder.commit_row();
                                        continue;
                                    }
                                }
                                convert_to_comparable(val, &mut builder.data);
                                builder.commit_row();
                            }
                            if data_type.is_nullable() {
                                Column::Nullable(Box::new(NullableColumn {
                                    column: Column::Variant(builder.build()),
                                    validity: validity.unwrap().clone(),
                                }))
                            } else {
                                Column::Variant(builder.build())
                            }
                        }
                        _ => c.clone(),
                    }
                }
            })
            .collect::<Vec<_>>();
        Ok(self.convert_columns(&columns, num_rows))
    }
}
