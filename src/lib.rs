use std::sync::mpsc;
use std::thread;
use bytes::Bytes;
use neon::{prelude::*, types::Deferred};
use rusqlite::Connection;
use tokio::runtime::Runtime;
use once_cell::sync::OnceCell;
use std::time::Duration;
use neon::handle::Managed;


//type DbCallback = Box<dyn FnOnce(&mut Connection, &Channel, Deferred) + Send>;
type DbCallback = Box<dyn FnOnce(&mut DbDropGuard, &Channel, Deferred) + Send>;
//mod Db;
//use crate::db;
mod db;
use crate::db::DbDropGuard;
use crate::db::Db;
struct Database {
    tx: mpsc::Sender<DbMessage>,
    db_holder: DbDropGuard,
}

// Messages sent on the database channel
enum DbMessage {
    // Promise to resolve and callback to be executed
    // Deferred is threaded through the message instead of moved to the closure so that it
    // can be manually rejected.
    Callback(Deferred, DbCallback),
    // Indicates that the thread should be stopped and connection closed
    Close,
}

// Clean-up when Database is garbage collected, could go here
// but, it's not needed,
impl Finalize for Database {}

// Internal implementation
impl Database {
    // Creates a new instance of `Database`
    //
    // 1. Creates a connection and a channel
    // 2. Spawns a thread and moves the channel receiver and connection to it
    // 3. On a separate thread, read closures off the channel and execute with access
    //    to the connection.
    fn new<'a, C>(cx: &mut C) -> rusqlite::Result<Self>
    where
        C: Context<'a>,
    {
        // Channel for sending callbacks to execute on the sqlite connection thread
        let (tx, rx) = mpsc::channel::<DbMessage>();

        // Start the background task.
        //let rt = runtime(&mut cx)?;
        
        let db_holder = DbDropGuard::new();
        // Open a connection sqlite, this will be moved to the thread
        let mut conn =  DbDropGuard::new(); //Connection::open_in_memory()?;

        // Create an `Channel` for calling back to JavaScript. It is more efficient
        // to create a single channel and re-use it for all database callbacks.
        // The JavaScript process will not exit as long as this channel has not been
        // dropped.
        let channel = cx.channel();

        // Spawn a thread for processing database queries
        // This will not block the JavaScript main thread and will continue executing
        // concurrently.
        thread::spawn(move || {
            // Blocks until a callback is available
            // When the instance of `Database` is dropped, the channel will be closed
            // and `rx.recv()` will return an `Err`, ending the loop and terminating
            // the thread.
            while let Ok(message) = rx.recv() {
                match message {
                    DbMessage::Callback(deferred, f) => {
                        // The connection and channel are owned by the thread, but _lent_ to
                        // the callback. The callback has exclusive access to the connection
                        // for the duration of the callback.
                        f(&mut conn, &channel, deferred);
                    }
                    // Immediately close the connection, even if there are pending messages
                    DbMessage::Close => break,
                }
            }
        });

        Ok(Self { tx, db_holder })
    }

    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    fn close(&self) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Close)
    }

    fn send(
        &self,
        deferred: Deferred,
        //callback: impl FnOnce(&mut Connection, &Channel, Deferred) + Send + 'static,
        callback: impl FnOnce(&mut DbDropGuard, &Channel, Deferred) + Send + 'static,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx
            .send(DbMessage::Callback(deferred, Box::new(callback)))
    }
}

// Methods exposed to JavaScript
// The `JsBox` boxed `Database` is expected as the `this` value on all methods except `js_new`
impl Database {
    // Create a new instance of `Database` and place it inside a `JsBox`
    // JavaScript can hold a reference to a `JsBox`, but the contents are opaque
    fn js_new(mut cx: FunctionContext) -> JsResult<JsBox<Database>> {
        let db = Database::new(&mut cx).or_else(|err| cx.throw_error(err.to_string()))?;

        Ok(cx.boxed(db))
    }

    // Manually close a database connection
    // After calling `close`, all other methods will fail
    // It is not necessary to call `close` since the database will be closed when the wrapping
    // `JsBox` is garbage collected. However, calling `close` allows the process to exit
    // immediately instead of waiting on garbage collection. This is useful in tests.
    fn js_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        cx.this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut cx)?
            .close()
            .or_else(|err| cx.throw_error(err.to_string()))?;

        Ok(cx.undefined())
    }

    // Inserts a `name` into the database
    // Accepts a `name` and returns a `Promise`
    fn js_insert(mut cx: FunctionContext) -> JsResult<JsPromise> {
        // Get the first argument as a `JsString` and convert to a Rust `String`
        let key = cx.argument::<JsString>(0)?.value(&mut cx);
        let value = cx.argument::<JsString>(1)?.value(&mut cx);
        let delays = cx.argument::<JsNumber>(2)?.value(&mut cx);

        // Get the `this` value as a `JsBox<Database>`
        let db = cx.this().downcast_or_throw::<JsBox<Database>, _>(&mut cx)?;
        let (deferred, promise) = cx.promise();

        db.send(deferred, move |conn, channel, deferred| {
            let ins_value = Bytes::from(value);
            let ins_key = key;
            let mls = Duration::from_millis(delays.ceil() as u64);    

            conn.db().set(ins_key, ins_value, Some(mls));
            deferred.settle_with(channel, move |mut cx| {
                let id = 1;

                Ok(cx.number(id as f64))
            });
        })
        .into_rejection(&mut cx)?;

        // This function does not have a return value
        Ok(promise)
    }

    // Get a `name` by `id` value
    // Accepts an `id` and callback as parameters
    fn js_get_by_id(mut cx: FunctionContext) -> JsResult<JsPromise> {
        // Get the first argument as a `JsNumber` and convert to an `f64`
        let key = cx.argument::<JsString>(0)?.value(&mut cx);

        // Get the `this` value as a `JsBox<Database>`
        let db = cx.this().downcast_or_throw::<JsBox<Database>, _>(&mut cx)?;
        let (deferred, promise) = cx.promise();

        db.send(deferred, move |conn, channel, deferred| {
            //let result : Result<String, _> = Ok((String::from("value")));
            let result = conn.db().get(&key);
            deferred.settle_with(channel, move |mut cx| -> JsResult<JsValue> {
                // If the row was not found, return `undefined` as a success instead
                // of throwing an exception
                let value = String::from_utf8(result.unwrap().to_vec()).unwrap();

                Ok(cx.string(value).upcast())
            });
        })
        .into_rejection(&mut cx)?;

        // This function does not have a return value
        Ok(promise)
    }

    // Get a `name` by `id` value
    // Accepts an `id` and callback as parameters
    fn js_get_by_exp(mut cx: FunctionContext) -> JsResult<JsPromise> {
        // Get the first argument as a `JsNumber` and convert to an `f64`
        //let key = cx.argument::<JsString>(0)?.value(&mut cx);

        // Get the `this` value as a `JsBox<Database>`
        let db = cx.this().downcast_or_throw::<JsBox<Database>, _>(&mut cx)?;
        let (deferred, promise) = cx.promise();

        db.send(deferred, move |conn, channel, deferred| {
            
            let result = conn.db().iter().unwrap();
            deferred.settle_with(channel, move |mut cx| -> JsResult<JsArray> {
                // If the row was not found, return `undefined` as a success instead
                // of throwing an exception

                let js_array = JsArray::new(&mut cx, result.len() as u32);
                for (i, record) in result.iter().enumerate() {
                    let value = cx.string(String::from_utf8(record.to_vec()).unwrap());
                    js_array.set(&mut cx, i as u32, value).unwrap();
                }
                Ok(js_array)
            });
        })
        .into_rejection(&mut cx)?;

        // This function does not have a return value
        Ok(promise)
    }
}

trait SendResultExt {
    // Sending a query closure to execute may fail if the channel has been closed.
    // This method converts the failure into a promise rejection.
    fn into_rejection<'a, C: Context<'a>>(self, cx: &mut C) -> NeonResult<()>;
}

impl SendResultExt for Result<(), mpsc::SendError<DbMessage>> {
    fn into_rejection<'a, C: Context<'a>>(self, cx: &mut C) -> NeonResult<()> {
        self.or_else(|err| {
            let msg = err.to_string();

            match err.0 {
                DbMessage::Callback(deferred, _) => {
                    let err = cx.error(msg)?;
                    deferred.reject(cx, err);
                    Ok(())
                }
                DbMessage::Close => cx.throw_error("Expected DbMessage::Callback"),
            }
        })
    }
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("databaseNew", Database::js_new)?;
    cx.export_function("databaseClose", Database::js_close)?;
    cx.export_function("databaseInsert", Database::js_insert)?;
    cx.export_function("databaseGetByKey", Database::js_get_by_id)?;
    cx.export_function("databaseGetByExp", Database::js_get_by_exp)?;

    Ok(())
}
