use crypto::aead::{AeadDecryptor, AeadEncryptor};
use crypto::aes_gcm::AesGcm;
use neon::prelude::*;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::ErrorKind;
use std::iter::repeat;
use std::str::from_utf8;
use std::sync::Arc;
use std::{collections::HashMap, fs, sync::Mutex};
use std::{io, str};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
enum TableValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Object(HashMap<String, TableValue>),
    Array(Vec<TableValue>),
}

#[derive(Serialize, Deserialize, Clone)]
struct Table {
    columns: Vec<String>,
    records: Vec<HashMap<String, TableValue>>,
}

struct Database {
    tables: HashMap<String, Table>,
    file_path: String,
    password: String,
    queue: Vec<QueueItem>,
}

struct QueueItem {
    method: String,
    table: String,
    record: Option<HashMap<String, TableValue>>,
    query: Option<HashMap<String, TableValue>>,
    new_data: Option<HashMap<String, TableValue>>,
}

impl Database {
    fn new(file_path: String, password: String) -> Self {
        Database {
            tables: HashMap::new(),
            file_path,
            password,
            queue: Vec::new(),
        }
    }

    fn create_table(&mut self, table_name: String, columns: Vec<String>) {
        self.read_from_file();
        if self.tables.contains_key(&table_name) {
            panic!("Table \"{}\" already exists.", table_name);
        }
        self.tables.insert(
            table_name,
            Table {
                columns,
                records: Vec::new(),
            },
        );
        self.save_to_file();
    }

    fn create_table_if_not_exists(&mut self, table_name: String, columns: Vec<String>) {
        self.read_from_file();
        if !self.tables.contains_key(&table_name) {
            self.tables.insert(
                table_name,
                Table {
                    columns,
                    records: Vec::new(),
                },
            );
            self.save_to_file();
        }
    }

    fn delete_table(&mut self, table_name: String) {
        self.read_from_file();
        if !self.tables.contains_key(&table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        self.tables.remove(&table_name);
        self.save_to_file();
    }

    fn delete_table_if_exists(&mut self, table_name: String) {
        self.read_from_file();
        if self.tables.contains_key(&table_name) {
            self.tables.remove(&table_name);
            self.save_to_file();
        }
    }

    fn add_column(
        &mut self,
        table_name: String,
        column: String,
        default_value: Option<TableValue>,
    ) {
        self.read_from_file();
        if !self.tables.contains_key(&table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = self.tables.get_mut(&table_name).unwrap();
        if table.columns.contains(&column) {
            panic!(
                "Column \"{}\" already exists in table \"{}\".",
                column, table_name
            );
        }
        table.columns.push(column.clone());
        let default_value = default_value.unwrap_or(TableValue::String(String::from("")));
        for record in &mut table.records {
            record.insert(column.clone(), default_value.clone());
        }
        self.save_to_file();
    }

    fn delete_column(&mut self, table_name: String, column: String) {
        self.read_from_file();
        if !self.tables.contains_key(&table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = self.tables.get_mut(&table_name).unwrap();
        if !table.columns.contains(&column) {
            panic!(
                "Column \"{}\" does not exist in table \"{}\".",
                column, table_name
            );
        }
        let index = table.columns.iter().position(|col| col == &column).unwrap();
        table.columns.remove(index);
        for record in &mut table.records {
            record.remove(&column);
        }
        self.save_to_file();
    }

    fn insert(&mut self, table_name: String, record: HashMap<String, TableValue>) {
        self.queue.push(QueueItem {
            method: "insert".to_string(),
            table: table_name,
            record: Some(record),
            query: None,
            new_data: None,
        });
        if self.queue.len() == 1 {
            self.process_queue();
        }
    }

    fn update(
        &mut self,
        table_name: String,
        query: HashMap<String, TableValue>,
        new_data: HashMap<String, TableValue>,
    ) {
        self.queue.push(QueueItem {
            method: "update".to_string(),
            table: table_name,
            record: None,
            query: Some(query),
            new_data: Some(new_data),
        });
        if self.queue.len() == 1 {
            self.process_queue();
        }
    }

    fn select(
        &mut self,
        table_name: String,
        query: Option<HashMap<String, TableValue>>,
    ) -> Vec<HashMap<String, TableValue>> {
        self.read_from_file();
        if !self.tables.contains_key(&table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = &self.tables[&table_name];
        if let Some(query_map) = query {
            table
                .records
                .iter()
                .filter(|record| {
                    query_map
                        .iter()
                        .all(|(column, value)| record[column] == *value)
                })
                .cloned()
                .collect()
        } else {
            table.records.clone()
        }
    }

    fn delete(&mut self, table_name: String, query: HashMap<String, TableValue>) {
        self.queue.push(QueueItem {
            method: "delete".to_string(),
            table: table_name,
            record: None,
            query: Some(query),
            new_data: None,
        });
        if self.queue.len() == 1 {
            self.process_queue();
        }
    }

    fn process_queue(&mut self) {
        if let Some(request) = self.queue.pop() {
            match &request.method[..] {
                "insert" => self.insert_table(&request.table, request.record.as_ref().unwrap()),
                "update" => self.update_table(
                    &request.table,
                    request.query.as_ref().unwrap(),
                    request.new_data.as_ref().unwrap(),
                ),
                "delete" => self.delete_from_table(&request.table, request.query.as_ref().unwrap()),
                _ => panic!("Invalid method: {}", request.method),
            }
        }
    }

    fn insert_table(&mut self, table_name: &String, record: &HashMap<String, TableValue>) {
        self.read_from_file();
        if !self.tables.contains_key(table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = self.tables.get_mut(table_name).unwrap();
        let formatted_record: HashMap<String, TableValue> = table
            .columns
            .iter()
            .map(|column| {
                (
                    column.clone(),
                    record
                        .get(column)
                        .cloned()
                        .unwrap_or(TableValue::String(String::from(""))),
                )
            })
            .collect();
        table.records.push(formatted_record);
        self.save_to_file();
        if !self.queue.is_empty() {
            self.queue.remove(0);
            self.process_queue();
        }
    }

    fn update_table(
        &mut self,
        table_name: &String,
        query: &HashMap<String, TableValue>,
        new_data: &HashMap<String, TableValue>,
    ) {
        self.read_from_file();
        if !self.tables.contains_key(table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = self.tables.get_mut(table_name).unwrap();
        table.records = table
            .records
            .iter()
            .map(|record| {
                if query.iter().all(|(column, value)| record[column] == *value) {
                    let updated_record: HashMap<String, TableValue> = record
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .chain(new_data.iter().map(|(k, v)| (k.clone(), v.clone())))
                        .collect();
                    updated_record
                } else {
                    record.clone()
                }
            })
            .collect();
        self.save_to_file();
        if !self.queue.is_empty() {
            self.queue.remove(0);
            self.process_queue();
        }
    }

    fn delete_from_table(&mut self, table_name: &String, query: &HashMap<String, TableValue>) {
        self.read_from_file();
        if !self.tables.contains_key(table_name) {
            panic!("Table \"{}\" does not exist.", table_name);
        }
        let table = self.tables.get_mut(table_name).unwrap();
        table
            .records
            .retain(|record| !query.iter().all(|(column, value)| record[column] == *value));
        self.save_to_file();
        if !self.queue.is_empty() {
            self.queue.remove(0);
            self.process_queue();
        }
    }

    fn read_tables(&mut self) -> HashMap<String, Table> {
        self.read_from_file();
        self.tables.clone()
    }

    fn save_to_file(&self) {
        if !self.file_path.ends_with(".ht") {
            panic!("File path must include '.ht': {}", self.file_path);
        }
        let data = serde_json::to_string(&self.tables).unwrap();
        let res = encrypt(data.as_bytes(), &self.password);
        fs::write(&self.file_path, res).unwrap();
    }

    fn read_from_file(&mut self) {
        if !fs::metadata(&self.file_path).is_ok() {
            return;
        }
        let encrypted = fs::read_to_string(&self.file_path).unwrap();
        let data = decrypt(encrypted.as_str(), &self.password).unwrap();
        let data_str = from_utf8(&data).unwrap();
        match serde_json::from_str::<HashMap<String, Table>>(&data_str) {
            Ok(tables) => self.tables = tables,
            Err(err) => {
                eprintln!("Error during deserialization: {:?}", err);
                self.tables = HashMap::new();
            }
        }
    }
}

fn split_iv_data_mac(orig: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let split: Vec<&str> = orig.split('/').into_iter().collect();

    if split.len() != 3 {
        return Err(Box::new(io::Error::from(ErrorKind::Other)));
    }
    let iv_res = hex::decode(split[0]);
    if iv_res.is_err() {
        return Err(Box::new(io::Error::from(ErrorKind::Other)));
    }
    let iv = iv_res.unwrap();

    let data_res = hex::decode(split[1]);
    if data_res.is_err() {
        return Err(Box::new(io::Error::from(ErrorKind::Other)));
    }
    let data = data_res.unwrap();

    let mac_res = hex::decode(split[2]);
    if mac_res.is_err() {
        return Err(Box::new(io::Error::from(ErrorKind::Other)));
    }
    let mac = mac_res.unwrap();

    Ok((iv, data, mac))
}

fn get_valid_key(key: &str) -> Vec<u8> {
    let mut bytes = key.as_bytes().to_vec();
    if bytes.len() < 16 {
        for _j in 0..(16 - bytes.len()) {
            bytes.push(0x00);
        }
    } else if bytes.len() > 16 {
        bytes = bytes[0..16].to_vec();
    }

    bytes
}

pub fn decrypt(iv_data_mac: &str, key: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let (iv, data, mac) = split_iv_data_mac(iv_data_mac)?;
    let key = get_valid_key(key);

    let key_size = crypto::aes::KeySize::KeySize128;
    let mut decipher = AesGcm::new(key_size, &key, &iv, &[]);
    let mut dst: Vec<u8> = repeat(0).take(data.len()).collect();
    let result = decipher.decrypt(&data, &mut dst, &mac);

    if result {}

    Ok(dst)
}

fn get_iv(size: usize) -> Vec<u8> {
    let mut iv = vec![];
    for _j in 0..size {
        let r = rand::random();
        iv.push(r);
    }

    iv
}

pub fn encrypt(data: &[u8], password: &str) -> String {
    let key_size = crypto::aes::KeySize::KeySize128;

    let valid_key = get_valid_key(password);
    let iv = get_iv(12);
    let mut cipher = AesGcm::new(key_size, &valid_key, &iv, &[]);

    let mut encrypted: Vec<u8> = repeat(0).take(data.len()).collect();

    let mut mac: Vec<u8> = repeat(0).take(16).collect();

    cipher.encrypt(data, &mut encrypted, &mut mac[..]);

    let hex_iv = hex::encode(iv);
    let hex_cipher = hex::encode(encrypted);
    let hex_mac = hex::encode(mac);
    let output = format!("{}/{}/{}", hex_iv, hex_cipher, hex_mac);

    output
}

fn create_table(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let columns = cx
        .argument::<JsArray>(1)?
        .to_vec(&mut cx)?
        .into_iter()
        .map(|val| val.downcast::<JsString, _>(&mut cx).unwrap().value(&mut cx))
        .collect();
    let mut db = database.lock().unwrap();
    db.create_table(table_name, columns);
    Ok(cx.undefined())
}

fn create_table_if_not_exists(
    mut cx: FunctionContext,
    database: Arc<Mutex<Database>>,
) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let columns = cx
        .argument::<JsArray>(1)?
        .to_vec(&mut cx)?
        .into_iter()
        .map(|val| val.downcast::<JsString, _>(&mut cx).unwrap().value(&mut cx))
        .collect();
    let mut db = database.lock().unwrap();
    db.create_table_if_not_exists(table_name, columns);
    Ok(cx.undefined())
}

fn delete_table(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let mut db = database.lock().unwrap();
    db.delete_table(table_name);
    Ok(cx.undefined())
}

fn delete_table_if_exists(
    mut cx: FunctionContext,
    database: Arc<Mutex<Database>>,
) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let mut db = database.lock().unwrap();
    db.delete_table_if_exists(table_name);
    Ok(cx.undefined())
}

fn add_column(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let column = cx.argument::<JsString>(1)?.value(&mut cx);
    let default_value = cx.argument_opt(2).map(|val| {
        convert_js_value_to_table_value(&mut cx, val)
            .expect("Failed to convert default value to TableValue")
    });
    let mut db = database.lock().unwrap();
    db.add_column(table_name, column, default_value);
    Ok(cx.undefined())
}

fn delete_column(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let column = cx.argument::<JsString>(1)?.value(&mut cx);
    let mut db = database.lock().unwrap();
    db.delete_column(table_name, column);
    Ok(cx.undefined())
}

fn convert_js_value_to_table_value(
    cx: &mut FunctionContext,
    value: Handle<JsValue>,
) -> NeonResult<TableValue> {
    if value.is_a::<JsString, _>(cx) {
        Ok(TableValue::String(
            value.downcast::<JsString, _>(cx).unwrap().value(cx),
        ))
    } else if value.is_a::<JsBoolean, _>(cx) {
        Ok(TableValue::Boolean(
            value.downcast::<JsBoolean, _>(cx).unwrap().value(cx),
        ))
    } else if value.is_a::<JsNumber, _>(cx) {
        Ok(TableValue::Integer(
            value.downcast::<JsNumber, _>(cx).unwrap().value(cx) as i64,
        ))
    } else if value.is_a::<JsArray, _>(cx) {
        let arr = value.downcast::<JsArray, _>(cx).unwrap();
        let mut arr_vec = Vec::new();
        let len = arr.len(cx);
        for i in 0..len {
            let value = arr.get(cx, i)?;
            let table_value = convert_js_value_to_table_value(cx, value)?;
            arr_vec.push(table_value);
        }
        Ok(TableValue::Array(arr_vec))
    } else if value.is_a::<JsObject, _>(cx) {
        let obj = value.downcast::<JsObject, _>(cx).unwrap();
        let mut obj_map = HashMap::new();
        let keys = obj.get_own_property_names(cx)?.to_vec(cx)?;
        for key in keys {
            let key_str = key.to_string(cx)?.value(cx);
            let value = obj.get(cx, key)?;
            let table_value = convert_js_value_to_table_value(cx, value)?;
            obj_map.insert(key_str, table_value);
        }
        Ok(TableValue::Object(obj_map))
    } else {
        panic!("Unsupported value type");
    }
}

fn insert(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let record_value = cx.argument::<JsValue>(1)?;
    let mut record: HashMap<String, TableValue> = HashMap::new();
    match record_value {
        value if value.is_a::<JsObject, _>(&mut cx) => {
            let record_obj = value.downcast::<JsObject, _>(&mut cx).unwrap();
            let keys = record_obj
                .get_own_property_names(&mut cx)?
                .to_vec(&mut cx)?;
            for key in keys {
                let key_str = key.to_string(&mut cx)?.value(&mut cx);
                let value = record_obj.get(&mut cx, key)?;
                let table_value = convert_js_value_to_table_value(&mut cx, value)?;
                record.insert(key_str, table_value);
            }
        }
        _ => panic!("Unsupported record type"),
    }
    let mut db = database.lock().unwrap();
    db.insert(table_name, record);
    Ok(cx.undefined())
}

fn update(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let query_obj = cx.argument::<JsObject>(1)?;
    let mut query: HashMap<String, TableValue> = HashMap::new();
    let keys = query_obj.get_own_property_names(&mut cx)?.to_vec(&mut cx)?;
    for key in keys {
        let key_str = key.to_string(&mut cx)?.value(&mut cx);
        let value = query_obj.get(&mut cx, key)?;
        let table_value = convert_js_value_to_table_value(&mut cx, value)?;
        query.insert(key_str, table_value);
    }
    let new_data_obj = cx.argument::<JsObject>(2)?;
    let mut new_data: HashMap<String, TableValue> = HashMap::new();
    let keys = new_data_obj
        .get_own_property_names(&mut cx)?
        .to_vec(&mut cx)?;
    for key in keys {
        let key_str = key.to_string(&mut cx)?.value(&mut cx);
        let value = new_data_obj.get(&mut cx, key)?;
        let table_value = convert_js_value_to_table_value(&mut cx, value)?;
        new_data.insert(key_str, table_value);
    }
    let mut db = database.lock().unwrap();
    db.update(table_name, query, new_data);
    Ok(cx.undefined())
}

fn select(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsValue> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let query_obj = cx.argument_opt(1);
    let mut query: Option<HashMap<String, TableValue>> = None;

    if let Some(query_obj) = query_obj {
        let query_obj = query_obj.downcast::<JsObject, _>(&mut cx);
        if let Ok(query_obj) = query_obj {
            let keys = query_obj.get_own_property_names(&mut cx)?.to_vec(&mut cx)?;
            let mut query_map = HashMap::new();
            for key in keys {
                let key_str = key.to_string(&mut cx)?.value(&mut cx);
                let value = query_obj
                    .get::<JsString, _, _>(&mut cx, key)?
                    .downcast::<JsString, _>(&mut cx);
                if let Ok(value) = value {
                    let value_str = value.value(&mut cx);
                    let table_value = TableValue::String(value_str);
                    query_map.insert(key_str, table_value);
                }
            }

            query = Some(query_map);
        }
    }

    let mut db = database.lock().unwrap();
    let result = db.select(table_name, query);
    let result = serde_json::to_string(&result).unwrap();
    Ok(cx.string(result).upcast())
}

fn delete(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsUndefined> {
    let table_name = cx.argument::<JsString>(0)?.value(&mut cx);
    let query_obj = cx.argument::<JsObject>(1)?;
    let mut query: HashMap<String, TableValue> = HashMap::new();
    let keys = query_obj.get_own_property_names(&mut cx)?.to_vec(&mut cx)?;
    for key in keys {
        let key_str = key.to_string(&mut cx)?.value(&mut cx);
        let value = query_obj
            .get::<JsString, _, _>(&mut cx, key)?
            .downcast::<JsString, _>(&mut cx);
        if let Ok(value) = value {
            let value_str = value.value(&mut cx);
            query.insert(key_str, TableValue::String(value_str));
        }
    }
    let mut db = database.lock().unwrap();
    db.delete(table_name, query);
    Ok(cx.undefined())
}

fn read_tables(mut cx: FunctionContext, database: Arc<Mutex<Database>>) -> JsResult<JsValue> {
    let mut db = database.lock().unwrap();
    let tables = db.read_tables();
    let result = serde_json::to_string(&tables).unwrap();
    Ok(cx.string(result).upcast())
}

fn init(mut cx: FunctionContext) -> JsResult<JsObject> {
    let exports = JsObject::new(&mut cx);
    let file_path = cx.argument::<JsString>(0)?.value(&mut cx);
    let password = cx.argument::<JsString>(1)?.value(&mut cx);
    let database = Arc::new(Mutex::new(Database::new(file_path, password)));
    let db = Arc::clone(&database);
    let db2 = Arc::clone(&database);
    let db3 = Arc::clone(&database);
    let db4 = Arc::clone(&database);
    let db5 = Arc::clone(&database);
    let db6 = Arc::clone(&database);
    let db7 = Arc::clone(&database);
    let db8 = Arc::clone(&database);
    let db9 = Arc::clone(&database);
    let db10 = Arc::clone(&database);
    let db11 = Arc::clone(&database);
    let create_table_function =
        JsFunction::new(&mut cx, move |cx| create_table(cx, Arc::clone(&db)));
    let create_table_if_not_exists_function = JsFunction::new(&mut cx, move |cx| {
        create_table_if_not_exists(cx, Arc::clone(&db2))
    });
    let delete_table_function =
        JsFunction::new(&mut cx, move |cx| delete_table(cx, Arc::clone(&db3)));
    let delete_table_if_exists_function = JsFunction::new(&mut cx, move |cx| {
        delete_table_if_exists(cx, Arc::clone(&db4))
    });
    let add_column_function = JsFunction::new(&mut cx, move |cx| add_column(cx, Arc::clone(&db5)));
    let delete_column_function =
        JsFunction::new(&mut cx, move |cx| delete_column(cx, Arc::clone(&db6)));
    let insert_function = JsFunction::new(&mut cx, move |cx| insert(cx, Arc::clone(&db7)));
    let update_function = JsFunction::new(&mut cx, move |cx| update(cx, Arc::clone(&db8)));
    let select_function = JsFunction::new(&mut cx, move |cx| select(cx, Arc::clone(&db9)));
    let delete_function = JsFunction::new(&mut cx, move |cx| delete(cx, Arc::clone(&db10)));
    let read_tables_function =
        JsFunction::new(&mut cx, move |cx| read_tables(cx, Arc::clone(&db11)));
    exports.set(&mut cx, "createTable", create_table_function?)?;
    exports.set(
        &mut cx,
        "createTableIfNotExists",
        create_table_if_not_exists_function?,
    )?;
    exports.set(&mut cx, "deleteTable", delete_table_function?)?;
    exports.set(
        &mut cx,
        "deleteTableIfExists",
        delete_table_if_exists_function?,
    )?;
    exports.set(&mut cx, "addColumn", add_column_function?)?;
    exports.set(&mut cx, "deleteColumn", delete_column_function?)?;
    exports.set(&mut cx, "insert", insert_function?)?;
    exports.set(&mut cx, "update", update_function?)?;
    exports.set(&mut cx, "select", select_function?)?;
    exports.set(&mut cx, "delete", delete_function?)?;
    exports.set(&mut cx, "readTables", read_tables_function?)?;
    Ok(exports)
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("init", init)?;
    Ok(())
}
