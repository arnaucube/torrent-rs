use async_std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::error::Error;
use std::net::Ipv4Addr;
use std::convert::TryInto;

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

// use rustc_hex::ToHex;

const CONNECT_MSG: u32 = 0;
const ANNOUNCE_MSG: u32 = 1;

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
    pub fn announce(&self) -> Option<String> {
        let mut a: String = "".to_string();
        match &self.announce {
            Some(s) => {
                a = s.to_string();
            }
            _ => (),
        };
        match &self.announce_list { // TODO refactor approach
            Some(list) => {
                a = list[0][0].clone();
            }
            _ => (),
        };
        Some(a)
    }
    pub fn protocol(&self) -> Option<String> {
        let announce = &self.announce().unwrap();
        let aux: Vec<&str> = announce.split(":").collect();
        let protocol = aux[0];
        Some(protocol.to_string())
    }
    pub async fn get_peers(&self) -> Result<Vec<Peer>, Box<dyn Error>> {
        // TODO timming system to resend request if no answer in X seconds
        if self.protocol().unwrap() != "udp" {
            panic!("not udp: {:?}", self.protocol().unwrap());
        }

        let peer = self.announce.clone().unwrap().replace("udp://", "");
        println!("peer {:?}", peer);
        let socket = UdpSocket::bind("0.0.0.0:6681").await?;

        let conn_req = conn_req_msg().to_vec();
        println!("SENDING conn_req {:?}", conn_req);
        socket.send_to(&conn_req, &peer).await?;

        let mut buf = vec![0; 1024];

        let peers = loop {
            let (n, src) = socket.recv_from(&mut buf).await?;

            let typ = resp_type(&buf);
            if typ == CONNECT_MSG {
                println!("TYPE: CONNECT: {:?}", CONNECT_MSG);
                // println!("HEX {:?}", &buf[0..n].to_hex());
                let conn_resp = parse_connect_resp(&buf[0..n].to_vec());
                let announce_req = ann_req_msg(conn_resp.connection_id, self, 6681);
                socket.send_to(&announce_req[..], &src).await?;
            } else if typ==ANNOUNCE_MSG {
                println!("TYPE: ANNOUNCE: {:?}", ANNOUNCE_MSG);
                // println!("HEX {:?}", &buf[0..n].to_hex());
                let ann_resp = parse_announce_resp(&buf[0..n].to_vec());
                break ann_resp.peers;
            }
        };

        Ok(peers)
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
    BigEndian::write_u32(&mut b[8..12], ANNOUNCE_MSG); // action
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[12..16].clone_from_slice(&random_bytes[..]);
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
    ip: String,
    port: u16
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
            // ip: BigEndian::read_u32(&b[20+(6*i)..24+(6*i)]),
            ip: bytes_to_ip(&b[20+(6*i)..24+(6*i)].try_into().expect("err parsing peer ip")),
            port: BigEndian::read_u16(&b[24+(6*i)..26+(6*i)]),
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
fn bytes_to_ip(b: &[u8; 4]) -> String {
    Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hex::ToHex;

    #[test]
    fn torrent_structs() {
        let t = Torrent::open("test.torrent");
        assert_eq!(t.info.name, "Big Buck Bunny");
        assert_eq!(t.announce().unwrap(), "udp://tracker.leechers-paradise.org:6969");
        assert_eq!(t.info_hash().to_hex(), "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert_eq!(t.size(), 276445467);
    }

    #[test]
    fn discovery_msgs() {
        let t = Torrent::open("test.torrent");

        let conn_req = conn_req_msg().to_vec();
        assert_eq!(conn_req[0..12].to_hex(), "000004172710198000000000"); // [12..16] is random
        let conn_resp_buf = b"00000000d4c575c2c078f9a83418bebc";
        let conn_resp = parse_connect_resp(&conn_resp_buf.to_vec());
        println!("conn_resp {:?}", conn_resp);
        assert_eq!(conn_resp.action, 808464432);
        assert_eq!(conn_resp.transaction_id, 808464432);
        assert_eq!(conn_resp.connection_id, 7220505182792409906);

        let announce_req = ann_req_msg(conn_resp.connection_id, &t, 6681);
        assert_eq!(announce_req[0..12].to_hex(), "643463353735633200000001");
        assert_eq!(announce_req[16..72].to_hex(), "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c0000000000000000000000000000000000000000000000000000000000000000107a391b");
        assert_eq!(announce_req[92..98].to_hex(), "ffffffff1a19");

        let announce_resp_buf = b"000000017a638a60000006e8000000030000002429d412a11ae12d53dcbd1a19d048c0e51ae1d8241960c8d5d5c36e71c8d5c5b974a31ae2c11ef348d4aabc5f370ec8d5bc4e16321ae1b9cee13b2327b94187b66f7cb94186b1c8d5b75308331ae1b068c0e0c263ac6f857911389d27ac821ae19088c2c01ae172fe2468c00a699aebe6720f68a2f99a1ae167fce2141ae163582ee41ae1634921a5d2725b72aaf5c8d559482e8fc9d257fb394cc8d55740e9ebd166524044e11aff51b6dcb6c8d54e2ebe61c3574b87860fc8d54b6e11026dff473afc7a1ec944cd47cd1ae131249b051ae124ff6b521ae105b75c13c8d50551153a1ae105024e09cd52";
        let ann_resp = parse_announce_resp(&announce_resp_buf.to_vec());
        assert_eq!(ann_resp.peers.len(), 81);
        assert_eq!(ann_resp.peers[0].ip, "48.54.101.56");
        assert_eq!(ann_resp.peers[0].port, 12336);
        assert_eq!(ann_resp.peers[80].ip, "52.101.48.57");
        assert_eq!(ann_resp.peers[80].port, 25444);
    }
    
    #[async_std::test]
    async fn get_peers() {
        let t = Torrent::open("test.torrent");
        // println!("{:?}", t);
        println!("torrent: {:?}", t.info.name);
        println!("protocol: {:?}", t.protocol().unwrap());
    
        println!("conn_req_msg: {:?}", conn_req_msg());
    
        let r = t.get_peers().await;
        println!("get_peers r: {:?}", r);
    }
}
