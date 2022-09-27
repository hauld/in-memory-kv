"use strict";

//const { promisify } = require("util");

const { databaseNew, databaseClose, databaseInsert, databaseGetByKey, databaseGetByExp } = require("./index.node");

// Wrapper class for the boxed `Database` for idiomatic JavaScript usage
class Database {
    constructor() {
        this.db = databaseNew();
    }

    // Wrap each method with a delegate to `this.db`
    // This could be node in several other ways, for example binding assignment
    // in the constructor
    set(key, value, delays) {
        return databaseInsert.call(this.db, key, value, delays);
    }

    get(key) {
        return databaseGetByKey.call(this.db, key);
    }

    iter() {
        return databaseGetByExp.call(this.db);
    }

    close() {
        databaseClose.call(this.db);
    }
}

module.exports = Database;
