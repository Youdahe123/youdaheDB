
// brings OpenOptions into scope so we can use it without the full path every time
use std::fs::OpenOptions;
// brings the Write trait into scope — needed to call write_all on a File
use std::io::Write;

// enum = a type that can be one of a fixed set of values
enum WalOperation {
    Put,
    Delete,
}

// struct = groups related fields together
struct WalRecord {
    key: String,
    value: String,
    operation: WalOperation, // which of the two enum variants this record is
}

struct Wal {
    // std::fs::File is the Rust type for an open file handle
    // storing it here means we open the file once and reuse it for every append
    file: std::fs::File,
}

// impl Wal = "here are the methods that belong to the Wal type"
impl Wal {

    // &str = a borrowed string reference — we don't need to own the path
    // -> Result<Wal, std::io::Error> = returns either a Wal on success or an io error on failure
    fn open(path: &str) -> Result<Wal, std::io::Error> {
        let file = OpenOptions::new()
            .read(true)    // allow reading from this file
            .append(true)  // all writes go to the end, never overwrite
            .create(true)  // create the file if it doesn't exist yet
            .open(path)?;  // actually open it — ? means "if this errors, return the error immediately"

        // Ok(...) wraps the value to signal success
        // Wal { file } creates a Wal struct with the file field set to our open file handle
        Ok(Wal { file })
    }

    // &mut self = we need mutable access because writing to the file changes its internal state
    // &WalRecord = we borrow the record, we don't need to own it
    fn append(&mut self, record: &WalRecord) -> Result<(), std::io::Error> {
        // match checks which variant record.operation is and returns a u8 value
        // the whole expression evaluates to 1 or 2 and gets stored in op_byte
        let op_byte = match record.operation {
            WalOperation::Put => 1u8,    // u8 = one byte unsigned integer
            WalOperation::Delete => 2u8,
        };

        // &[op_byte] = a one-element slice of bytes — write_all expects &[u8]
        self.file.write_all(&[op_byte])?;

        // .len() = number of characters in the key
        // as u32 = cast to a 32-bit integer so it's always exactly 4 bytes
        // .to_le_bytes() = convert that u32 to 4 bytes in little-endian order
        self.file.write_all(&(record.key.len() as u32).to_le_bytes())?;

        // .as_bytes() = converts the String to a raw byte slice so we can write it
        self.file.write_all(record.key.as_bytes())?;

        // same pattern for the value
        self.file.write_all(&(record.value.len() as u32).to_le_bytes())?;
        self.file.write_all(record.value.as_bytes())?;

        // Ok(()) = success with no return value — () is like void
        Ok(())
    }

    fn recover(path: &str) -> Result<Vec<WalRecord>, std::io::Error> {
        todo!("recover function")
    }
}
