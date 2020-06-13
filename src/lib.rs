use byteorder::{BigEndian, ByteOrder};
use rand::Rng;
use sha1::{Digest, Sha1};

extern crate serde;
extern crate serde_bencode;
#[macro_use]
extern crate serde_derive;
extern crate serde_bytes;

use serde_bencode::{de, ser};
use serde_bytes::ByteBuf;

#[derive(Debug, Deserialize)]
struct Node(String, i64);

#[derive(Debug, Serialize, Deserialize)]
struct File {
    path: Vec<String>,
    length: i64,
    #[serde(default)]
    md5sum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Info {
    name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length")]
    piece_length: i64,
    #[serde(default)]
    md5sum: Option<String>,
    #[serde(default)]
    length: Option<i64>,
    #[serde(default)]
    files: Option<Vec<File>>,
    #[serde(default)]
    private: Option<u8>,
    #[serde(default)]
    path: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "root hash")]
    root_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Torrent {
    info: Info,
    #[serde(default)]
    announce: Option<String>,
    #[serde(default)]
    nodes: Option<Vec<Node>>,
    #[serde(default)]
    encoding: Option<String>,
    #[serde(default)]
    httpseeds: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "announce-list")]
    announce_list: Option<Vec<Vec<String>>>,
    #[serde(default)]
    #[serde(rename = "creation date")]
    creation_date: Option<i64>,
    #[serde(rename = "comment")]
    comment: Option<String>,
    #[serde(default)]
    #[serde(rename = "created by")]
    created_by: Option<String>,
}

impl Torrent {
    pub fn open(path: &str) -> Torrent {
        let b = std::fs::read(path).unwrap();
        de::from_bytes::<Torrent>(&b).unwrap()
    }
    pub fn size(&self) -> u64 {
        let files = self.info.files.as_ref().unwrap();
        let mut size: u64 = 0;
        for i in 0..files.len() {
            size = size + files[i].length as u64;
        }
        size
    }
    pub fn info_hash(&self) -> Vec<u8> {
        let info_bytes: Vec<u8> = ser::to_bytes(&self.info).unwrap();
        let mut hasher = Sha1::new();
        hasher.update(info_bytes);
        let result = hasher.finalize();
        result.to_vec()
    }
}

const CONNECT_MSG: u32 = 0;
const ANNOUNCE_MSG: u32 = 1;

fn conn_req_msg() -> [u8; 16] {
    let mut b: [u8; 16] = [0; 16];
    BigEndian::write_u64(&mut b[0..8], 0x41727101980); // connection_id
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[12..16].clone_from_slice(&random_bytes[..]);
    b
}

fn ann_req_msg(connection_id: u64, torrent: &Torrent, port: u16) -> Vec<u8> {
    let mut b: [u8; 98] = [0; 98];
    BigEndian::write_u64(&mut b[0..8], connection_id);
    BigEndian::write_u32(&mut b[8..12], CONNECT_MSG); // action
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[12..16].clone_from_slice(&random_bytes[..]);
    // println!("md5sum {:?}", torrent);
    b[16..36].clone_from_slice(&torrent.info_hash()[..]);
    println!("info_hash: {:?}", &b[16..36]);
    // TODO [36..56] peerId b[36..56].clone_from_slice("todo".as_bytes());
    // TODO [56..64] downloaded
    println!("torrent.size(): {:?}", torrent.size());
    BigEndian::write_u64(&mut b[64..72], torrent.size()); // [64..72] left
                                                          // TODO [72..80] uploaded
                                                          // TODO [80..84] event (0:none, 1:completed, 2:started, 3:stopped)
                                                          // TODO [84..88] ip address
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[88..92].clone_from_slice(&random_bytes[..]); // [88..92] key (random)
    BigEndian::write_i32(&mut b[92..96], -1); // [92..96] num_want (default: -1)
    BigEndian::write_u16(&mut b[96..98], port); // [96..98] port
    b.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        let t = Torrent::open("test.torrent");
        println!("torrent: {:?}", t.info.name);
        println!("announce: {:?}", t.announce.clone().unwrap());

        println!("conn_req_msg: {:?}", conn_req_msg());
    }
}
