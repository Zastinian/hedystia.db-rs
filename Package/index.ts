const db = require("./index.node");

interface Table {
  columns: string[];
  records: {
    [key: string]: any;
  }[];
}

interface Record {
  [key: string]: string;
}

interface Query {
  [key: string]: string;
}

type CreateTableFunction = (tableName: string, columns: string[]) => void;
type CreateTableIfNotExistsFunction = (tableName: string, columns: string[]) => void;
type DeleteTableFunction = (tableName: string) => void;
type DeleteTableIfExistsFunction = (tableName: string) => void;
type AddColumnFunction = (tableName: string, column: string, defaultValue: string) => void;
type DeleteColumnFunction = (tableName: string, column: string) => void;
type InsertFunction = (tableName: string, record: Record) => void;
type UpdateFunction = (tableName: string, query: Query, newData: Record) => void;
type SelectFunction = (tableName: string, query?: Query) => string;
type DeleteFunction = (tableName: string, query: Query) => void;
type ReadTablesFunction = () => string;

interface DatabaseInterface {
  createTable: CreateTableFunction;
  createTableIfNotExists: CreateTableIfNotExistsFunction;
  deleteTable: DeleteTableFunction;
  deleteTableIfExists: DeleteTableIfExistsFunction;
  addColumn: AddColumnFunction;
  deleteColumn: DeleteColumnFunction;
  insert: InsertFunction;
  update: UpdateFunction;
  select: SelectFunction;
  delete: DeleteFunction;
  readTables: ReadTablesFunction;
}

export default class Database {
  db: DatabaseInterface;
  constructor(file_path: string, password: string) {
    this.db = db.init(file_path, password) as DatabaseInterface;
  }
  createTable(tableName: string, columns: string[]) {
    this.db.createTable(tableName, columns);
  }
  createTableIfNotExists(tableName: string, columns: string[]) {
    this.db.createTableIfNotExists(tableName, columns);
  }
  deleteTable(tableName: string): void {
    this.db.deleteTable(tableName);
  }
  deleteTableIfExists(tableName: string): void {
    this.db.deleteTableIfExists(tableName);
  }
  addColumn(tableName: string, column: string, defaultValue?: string): void {
    if (!defaultValue) {
      defaultValue = "";
    }
    this.db.addColumn(tableName, column, defaultValue);
  }
  deleteColumn(tableName: string, column: string) {
    this.db.deleteColumn(tableName, column);
  }
  insert(tableName: string, record: Record): void {
    this.db.insert(tableName, record);
  }
  update(tableName: string, query: Query, newData: Record): void {
    this.db.update(tableName, query, newData);
  }
  select(tableName: string, query?: Query): object {
    return JSON.parse(this.db.select(tableName, query));
  }
  delete(tableName: string, query: Query): void {
    this.db.delete(tableName, query);
  }
  readTables(): { [key: string]: Table } {
    return JSON.parse(this.db.readTables());
  }
}
