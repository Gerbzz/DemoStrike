//! Header/stat checksum used by CS Demo Manager; malformed and unsupported protobuf encodings fail closed.
use std::{io::Read,path::Path};
#[derive(Debug,PartialEq,Eq)]
pub enum ChecksumError { Io(String), InvalidHeader, Source2Header(String) }
impl std::fmt::Display for ChecksumError {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self {Self::Io(e)=>write!(f,"{e}"),Self::InvalidHeader=>write!(f,"Invalid demo header"),Self::Source2Header(e)=>write!(f,"Invalid demo header: {e}")}}}
fn varint(data:&[u8],offset:&mut usize)->Result<u64,ChecksumError>{let mut value=0u64;for i in 0..10{let b=*data.get(*offset).ok_or(ChecksumError::InvalidHeader)?;*offset+=1;if i==9&&b>1{return Err(ChecksumError::InvalidHeader)}value|=((b&127)as u64)<<(7*i);if b&128==0{return Ok(value)}}Err(ChecksumError::InvalidHeader)}
fn read_tag(data:&[u8],offset:&mut usize)->Result<u64,ChecksumError>{let start=*offset;let value=varint(data,offset)?;if *offset-start>5||value>u32::MAX as u64{return Err(ChecksumError::InvalidHeader)}Ok(value)}
fn frame_varint(data:&[u8],offset:&mut usize)->Result<i32,ChecksumError>{let mut value=0u32;for i in 0..5{let b=*data.get(*offset).ok_or(ChecksumError::InvalidHeader)?;*offset+=1;value|=((b&127)as u32).wrapping_shl(7*i);if b&128==0{break}}Ok(value as i32)}
// Unknown protobuf groups are ignored by the original decoder, with matching end tags.
fn skip_group(data:&[u8],offset:&mut usize,field:u64)->Result<(),ChecksumError>{
 let mut stack=vec![field];
 while !stack.is_empty(){let tag=read_tag(data,offset)?;let field=tag>>3;let wire=tag&7;if field==0{return Err(ChecksumError::InvalidHeader)}match wire {
  0=>{varint(data,offset)?;},
  1=>{*offset=offset.checked_add(8).filter(|n|*n<=data.len()).ok_or(ChecksumError::InvalidHeader)?;},
  2=>{let len=usize::try_from(varint(data,offset)?).map_err(|_|ChecksumError::InvalidHeader)?;*offset=offset.checked_add(len).filter(|n|*n<=data.len()).ok_or(ChecksumError::InvalidHeader)?;},
  3=>{if stack.len()>=100{return Err(ChecksumError::InvalidHeader)}stack.push(field);},
  4=>{if stack.pop()!=Some(field){return Err(ChecksumError::InvalidHeader)}},
  5=>{*offset=offset.checked_add(4).filter(|n|*n<=data.len()).ok_or(ChecksumError::InvalidHeader)?;},
  _=>return Err(ChecksumError::InvalidHeader),
 }}Ok(())
}
pub(crate) fn source2_data(buf:&[u8;4096])->Result<(String,String,String,i32,i32,String,String),ChecksumError>{
 let mut i=16;if frame_varint(buf,&mut i)?!=1{return Err(ChecksumError::Source2Header("Unexpected first proto message type".into()))}frame_varint(buf,&mut i)?;let len=frame_varint(buf,&mut i)?;let len=usize::try_from(len).map_err(|_|ChecksumError::InvalidHeader)?;
 // Original BitStream's refill mutates the backing buffer near its end; fail closed there.
 let end=i.checked_add(len).filter(|n|*n<=4088).ok_or(ChecksumError::InvalidHeader)?;let msg=&buf[i..end];let mut pos=0;let mut fields:[String;5]=Default::default();let(mut patch,mut build)=(0i32,0i32);
 while pos<msg.len(){let tag=read_tag(msg,&mut pos)?;let(field,wire)=(tag>>3,tag&7);if field==0{return Err(ChecksumError::InvalidHeader)}let expected=match field{1|3|4|5|6|10|11|12|14=>Some(2),2|7|8|9|13|15=>Some(0),_=>None};if expected.is_some_and(|w|w!=wire){return Err(ChecksumError::InvalidHeader)}match wire{
  0=>{let n=varint(msg,&mut pos)? as u32 as i32;if field==2{patch=n}else if field==13{build=n}},
  1=>{pos=pos.checked_add(8).filter(|n|*n<=msg.len()).ok_or(ChecksumError::InvalidHeader)?},
  2=>{let len=usize::try_from(varint(msg,&mut pos)?).map_err(|_|ChecksumError::InvalidHeader)?;let end=pos.checked_add(len).filter(|n|*n<=msg.len()).ok_or(ChecksumError::InvalidHeader)?;let slot=match field{3=>Some(0),4=>Some(1),5=>Some(2),11=>Some(3),12=>Some(4),_=>None};if let Some(n)=slot{fields[n]=String::from_utf8_lossy(&msg[pos..end]).into_owned()}pos=end},
  3=>{skip_group(msg,&mut pos,field)?},
  5=>{pos=pos.checked_add(4).filter(|n|*n<=msg.len()).ok_or(ChecksumError::InvalidHeader)?},
  _=>return Err(ChecksumError::InvalidHeader),
 }}
 if patch==0||build==0||fields.iter().any(String::is_empty){return Err(ChecksumError::Source2Header("Missing required protobuf fields".into()))}
 Ok((fields[2].clone(),fields[0].replace('\u{fffd}',""),fields[1].replace('\u{fffd}',""),patch,build,fields[4].clone(),fields[3].clone()))
}
pub fn checksum(path:&Path)->Result<String,ChecksumError>{
 let mut f=std::fs::File::open(path).map_err(|e|ChecksumError::Io(e.to_string()))?;let mut b=[0u8;4096];f.read(&mut b).map_err(|e|ChecksumError::Io(e.to_string()))?;let size=f.metadata().map_err(|e|ChecksumError::Io(e.to_string()))?.len();checksum_buffer(&b,size)
}
pub(crate) fn checksum_buffer(b:&[u8;4096],size:u64)->Result<String,ChecksumError>{let stamp=String::from_utf8_lossy(&b[..8]).replace('\0',"");
 let data=match stamp.as_str(){"HL2DEMO"=>{let text=|r:std::ops::Range<usize>|String::from_utf8_lossy(&b[r]).replace('\0',"");let n=|o:usize|u32::from_le_bytes(b[o..o+4].try_into().expect("fixed slice"));format!("{}{}{}{}{}{}{}",text(536..796),text(16..276),text(276..536),n(1064),n(1060),n(12),n(1068))},"PBDEMS2"=>{let(map,server,client,patch,build,guid,version)=source2_data(b)?;format!("{map}{server}{client}{patch}{build}{guid}{version}")},_=>return Err(ChecksumError::InvalidHeader)};
 let data=format!("{data}{}",js_size(size));let mut crc=u64::MAX;for byte in data.bytes(){crc^=u64::from(byte);for _ in 0..8{crc=if crc&1==1{(crc>>1)^0xc96c5795d7870f42}else{crc>>1}}}Ok(format!("{:x}",!crc))
}
pub(crate) fn js_size(size:u64)->String{(size as f64).to_string()}



