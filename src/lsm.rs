
use std::fs::OpenOptions;

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
        todo!("append function")
    }
    
    fn recover(path:&str) -> Result<Vec<WalRecord>, std::io::Error>{
        todo!("recover function")
    }
}