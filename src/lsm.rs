
use std::fs::OpenOptions;
use std::io::Write;

enum WalOperation {
    Put ,
    Delete,
}

struct WalRecord {
    key : String,
    value : String,
    operation : WalOperation,
}

struct Wal{
    file : std::fs::File,
}

impl Wal { 


    fn open(path:&str) -> Result<Wal, std::io::Error> {
        let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;

        Ok(Wal{file})
    }


    fn append(&mut self, record : &WalRecord)->  Result<(), std::io::Error>{
        let op_byte = match record.operation{
            WalOperation::Put => 1u8,
            WalOperation::Delete => 2u8,
        };
        self.file.write_all(&[op_byte])?;
        Ok(())
    }
    
    fn recover(path:&str) -> Result<Vec<WalRecord>, std::io::Error>{
        todo!("recover function")
    }
}