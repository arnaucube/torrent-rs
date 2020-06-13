use async_std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::error::Error;

use byteorder::{BigEndian, ByteOrder};
use rand::Rng;
use sha1::{Sha1, Digest};

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
    pub fn protocol(&self) -> Option<&str> {
        match &self.announce {
            Some(s) => {
                let aux: Vec<&str> = s.split(":").collect();
                Some(aux[0])
            }
            _ => None,
        }
    }
    // pub fn peer(&self) -> Option<&str> {
    //     match &self.announce {
    //         Some(s) => {
    //             let aux: Vec<&str> = s.split(":").collect();
    //             Some(aux[1])
    //         }
    //         _ => None,
    //     }
    // }
    pub async fn get_peers(&self) -> Result<(), Box<dyn Error>> {
        if self.protocol().unwrap() != "udp" {
            panic!("not udp: {:?}", self.protocol().unwrap());
        }

        let peer = self.announce.clone().unwrap().replace("udp://", "");
        println!("peer {:?}", peer);
        let socket = UdpSocket::bind("0.0.0.0:6681").await?;

        let conn_req = conn_req_msg().to_vec();
        println!("SENDING conn_req {:?}", conn_req);
        socket.send_to(&conn_req, &peer).await?;

        // let mut res: Vec<u8>;
        let mut buf = vec![0; 1024];

        let peers = loop {
            let (n, src) = socket.recv_from(&mut buf).await?;

            let typ = resp_type(&buf);
            println!("res {:?}", &buf[0..n]);
            println!("t {:?}", typ);
            // match typ {
            if typ == CONNECT_MSG {
                println!("TYPE: CONNECT: {:?}", CONNECT_MSG);
                let conn_resp = parse_connect_resp(&buf);
                println!("conn_resp {:?}", conn_resp);
                let announce_req = ann_req_msg(conn_resp.connection_id, self, 6681);
                println!("announce_req {:?}", announce_req);
                socket.send_to(&announce_req[..], &src).await?;
            } else if typ==ANNOUNCE_MSG {
                println!("TYPE: ANNOUNCE: {:?}", ANNOUNCE_MSG);
                let ann_resp = parse_announce_resp(&buf);
                println!("PEERS1 {:?}", ann_resp.peers);
                break ann_resp.peers;
            }
            println!("End");
        };
        println!("PEERS2 {:?}", peers);

        Ok(())
    }
}

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

const CONNECT_MSG: u32 = 0;
const ANNOUNCE_MSG: u32 = 1;
fn resp_type(b: &Vec<u8>) -> u32 {
    let action = BigEndian::read_u32(&b[0..4]);
    if action == 0 {
        return CONNECT_MSG;
    } else {
        return ANNOUNCE_MSG;
    }
}

#[derive(Debug)]
struct ConnResp {
    action: u32,
    transaction_id: u32,
    connection_id: u64,
}
fn parse_connect_resp(b: &Vec<u8>) -> ConnResp {
    ConnResp {
        action: BigEndian::read_u32(&b[0..4]),
        transaction_id: BigEndian::read_u32(&b[4..8]),
        connection_id: BigEndian::read_u64(&b[8..]),
    }
}

#[derive(Debug)]
struct Peer {
    ip: u32,
    port: u32
}

#[derive(Debug)]
struct AnnResp {
    action: u32,
    transaction_id: u32,
    interval: u32,
    leechers: u32,
    seeders: u32,
    peers: Vec<Peer>,
}
fn parse_announce_resp(b: &Vec<u8>) -> AnnResp {
    let mut peers: Vec<Peer> = Vec::new();
    let n_peers = (b.len()-20)/6;
    for i in 0..n_peers {
        let peer: Peer = Peer {
            ip: BigEndian::read_u32(&b[20+(6*i)..24+(6*i)]),
            port: BigEndian::read_u32(&b[24+(6*i)..26+(6*i)]),
        };
        peers.push(peer);
    }

    let ann_resp: AnnResp = AnnResp {
        action: BigEndian::read_u32(&b[0..4]),
        transaction_id: BigEndian::read_u32(&b[4..8]),
        interval: BigEndian::read_u32(&b[8..12]),
        leechers: BigEndian::read_u32(&b[12..16]),
        seeders: BigEndian::read_u32(&b[16..20]),
        peers: peers
    };

    ann_resp
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    #[async_std::test]
    async fn test_read() {
        let t = Torrent::open("test.torrent");
        // println!("{:?}", t);
        println!("torrent: {:?}", t.info.name);
        println!("announce: {:?}", t.announce.clone().unwrap());
        println!("protocol: {:?}", t.protocol().unwrap());

        println!("conn_req_msg: {:?}", conn_req_msg());

        let r = t.get_peers().await;
        println!("get_peers r: {:?}", r);
    }
}
