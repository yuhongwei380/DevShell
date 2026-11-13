use base64::Engine;

#[derive(Clone,Default)]
pub(super) struct ShellIntegration {
    nonce:String,
    pub ready:bool,
    pub at_prompt:bool,
    state:u8,
    payload:Vec<u8>,
}
#[derive(Debug,PartialEq)]
pub(super) enum ShellEvent {Prompt,Command(String),Directory(String)}
impl ShellIntegration {
    pub fn accept(&mut self,metadata:&serde_json::Value){
        if let Some(nonce)=metadata["nonce"].as_str().filter(|n|n.len()==48&&n.bytes().all(|b|b.is_ascii_hexdigit())){self.nonce=nonce.into();}
    }
    pub fn feed(&mut self,bytes:&[u8])->Vec<ShellEvent>{
        let mut events=Vec::new();
        for &byte in bytes {
            match (self.state,byte) {
                (0,0x1b)=>self.state=1,
                (1,b']')=>{self.payload.clear();self.state=2;},
                (1,0x1b)=>{},(1,_)=>self.state=0,
                (2,7)|(3,b'\\')=>{if let Some(event)=self.finish(){events.push(event);}self.payload.clear();self.state=0;},
                (2,0x1b)=>self.state=3,
                (2,_)=>{if self.payload.len()<90064{self.payload.push(byte);}else{self.payload.clear();self.state=4;}},
                (3,_)=>{self.payload.clear();self.state=0;},
                (4,7)|(5,b'\\')=>self.state=0,
                (4,0x1b)=>self.state=5,(5,_)=>self.state=4,
                _=>{},
            }
        }events
    }
    fn finish(&mut self)->Option<ShellEvent>{
        let payload=std::str::from_utf8(&self.payload).ok()?;let parts=payload.split(';').collect::<Vec<_>>();
        if parts.len()<4||parts[0]!="777"||parts[1]!="DengShell"||self.nonce.is_empty()||parts[3]!=self.nonce{return None;}
        match parts[2]{
            "ready"=>self.ready=true,
            "prompt"=>{self.at_prompt=true;return Some(ShellEvent::Prompt);},
            "busy"=>self.at_prompt=false,
            "command"|"cwd" if parts.len()==5=>{
                let value=base64::engine::general_purpose::STANDARD.decode(parts[4]).ok()?;
                let value=String::from_utf8(value).ok()?;if value.len()>16384||value.contains('\0'){return None;}
                if parts[2]=="command"{return Some(ShellEvent::Command(value));}
                if value.starts_with('/'){return Some(ShellEvent::Directory(value));}
            },_=>{},
        }None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_reports_require_the_current_authenticated_nonce(){
        let nonce="a".repeat(48);let mut parser=ShellIntegration::default();parser.accept(&serde_json::json!({"nonce":nonce}));
        assert!(parser.feed(b"\x1b]777;DengShell;prompt;forged\x07").is_empty());assert!(!parser.at_prompt);
        let report=format!("\x1b]777;DengShell;ready;{nonce};bash\x07\x1b]777;DengShell;cwd;{nonce};{}\x1b\\\x1b]777;DengShell;prompt;{nonce}\x07",base64::engine::general_purpose::STANDARD.encode("/home/中文"));
        let events=report.as_bytes().chunks(3).flat_map(|chunk|parser.feed(chunk)).collect::<Vec<_>>();
        assert_eq!(events,vec![ShellEvent::Directory("/home/中文".into()),ShellEvent::Prompt]);assert!(parser.ready&&parser.at_prompt);
        parser.feed(format!("\x1b]777;DengShell;busy;{nonce}\x07").as_bytes());assert!(!parser.at_prompt);
        let oversized=format!("\x1b]{}\x07", "x".repeat(100000));assert!(parser.feed(oversized.as_bytes()).is_empty());assert!(parser.payload.len()<90065);
        assert_eq!(parser.feed(format!("\x1b]777;DengShell;prompt;{nonce}\x07").as_bytes()),vec![ShellEvent::Prompt]);
    }
}
