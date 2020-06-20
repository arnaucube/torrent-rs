use async_std::net::{SocketAddr, ToSocketAddrs, UdpSocket, TcpStream};
use async_std::prelude::*;
use async_std::io;
use async_std::task::spawn;
// use std::io::prelude::*;
// use std::net::TcpStream;
use std::error::Error;
use std::net::Ipv4Addr;
use std::convert::TryInto;
use std::str;

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
const BLOCK_LEN: u32 = 16384; // 2**14

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
    pub fn piece_len(&self, piece_index: u32) -> u32 {
        let total_len = self.size() as u32;
        let piece_len = self.info.piece_length as u32;

        let last_piece_len = total_len % piece_len;
        let last_piece_index = ((total_len / piece_len) as f32).floor() as u32;
        if last_piece_index == piece_index {
            return last_piece_len;
        }
        piece_len
    }
    pub fn blocks_per_piece(&self, piece_index: u32) -> u32 {
        let piece_len = self.piece_len(piece_index);
        ((piece_len as u32 / BLOCK_LEN) as f32).floor() as u32
    }
    pub fn block_len(&self, piece_index: u32, block_index: u32) -> u32 {
        let piece_len = self.piece_len(piece_index);
        let last_piece_len = piece_len % BLOCK_LEN;
        let last_piece_index = ((piece_len / BLOCK_LEN) as f32).floor() as u32;
        if block_index==last_piece_index {
            return last_piece_len;
        }
        BLOCK_LEN
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
            panic!("not udp: {:?}", self.protocol().unwrap()); // TODO remove panic, return error
        }

        let peer = self.announce.clone().unwrap().replace("udp://", "");
        println!("peer {:?}", peer);
        let socket = UdpSocket::bind("0.0.0.0:6681").await?;

        let conn_req = msg_conn_req().to_vec();
        println!("SENDING conn_req {:?}", conn_req);
        socket.send_to(&conn_req, &peer).await?;

        let mut buf = vec![0u8; 1024];

        let peers = loop {
            let (n, src) = socket.recv_from(&mut buf).await?;

            let typ = msg_resp_type(&buf);
            if typ == CONNECT_MSG {
                println!("TYPE: CONNECT: {:?}", CONNECT_MSG);
                // println!("HEX {:?}", &buf[0..n].to_hex());
                let conn_resp = msg_parse_connect_resp(&buf[0..n].to_vec());
                let announce_req = msg_ann_req(conn_resp.connection_id, self, 6681);
                socket.send_to(&announce_req[..], &src).await?;
            } else if typ==ANNOUNCE_MSG {
                println!("TYPE: ANNOUNCE: {:?}", ANNOUNCE_MSG);
                // println!("HEX {:?}", &buf[0..n].to_hex());
                let ann_resp = msg_parse_announce_resp(&buf[0..n].to_vec());
                break ann_resp.peers;
            }
        };

        Ok(peers)
    }

}


struct Block {
    length: u32,
    bytes: Vec<u8>,
}
struct Payload {
    index: u32,
    begin: u32,
    length: u32,
    block: Block,
}
struct Msg {
    size: u32,
    id: u8,
    payload: Payload,
}

fn msg_parse(b: Vec<u8>) -> Result<Msg, &'static str> {
    if b.len()<4 {
        return Err("No id in msg.");
    }
    let id = b[4];
    if b.len()<5 {
        return Err("No payload in msg.");
    }
    let mut payload: Payload = Payload{
        index: 0,
        begin: 0,
        length: 0,
        block: Block{
            length: 0,
            bytes: Vec::new()
        }
    };
    match id {
        6 | 7 | 8 => {
            payload.index = BigEndian::read_u32(&b[5..9]);
            payload.begin = BigEndian::read_u32(&b[9..13]);
            if id==7 {
                payload.length = BigEndian::read_u32(&b[13..]);
            }
        }
        _ => ()
    };
    Ok(Msg{
        size: BigEndian::read_u32(&b[0..4]),
        id: id,
        payload: payload,
    })
}

async fn msg_handler<'a>(
        b: Vec<u8>,
        mut stream: TcpStream,
        torrent: &Torrent,
        pieces: &mut Pieces,
        queue: &mut Queue<'a>,
        file: String,
    ) -> Result<(), Box<dyn Error>> {
    if is_handshake(&b) {
        stream.write(&msg_interested()).await?;
        println!("IS HANDSHAKE");
        return Ok(())
    }
    let m = msg_parse(b).unwrap();
    match m.id {
        0 => handler_choke(stream),
        1 => handler_unchoke(stream, pieces, queue),
        4 => handler_have(stream, pieces, queue, m.payload),
        5 => handler_bitfield(stream, pieces, queue, m.payload),
        7 => handler_piece(stream, pieces, queue, torrent, file, m.payload),
        _ => {},
    }
    Ok(())
}

fn is_handshake(b: &Vec<u8>) -> bool {
    b.len()==(b[0] as usize) +49 && b.len()==68 && &b[1..20] == b"BitTorrent protocol"
}

fn handler_choke(mut stream: TcpStream) {
    // stream.shutdown()
}
fn handler_unchoke(mut stream: TcpStream, pieces: &mut Pieces, queue: &mut Queue) {
    queue.chocked = false;
    request_piece(stream, pieces, queue);

}
fn handler_have(mut stream: TcpStream, pieces: &mut Pieces, queue: &mut Queue, payload: Payload) {

}
fn handler_bitfield(mut stream: TcpStream, pieces: &mut Pieces, queue: &mut Queue, payload: Payload) {

}
fn handler_piece(mut stream: TcpStream, pieces: &mut Pieces, queue: &mut Queue, torrent: &Torrent, file: String, payload: Payload) {

}
async fn request_piece<'a>(mut stream: TcpStream, pieces: &mut Pieces, queue: &mut Queue<'a>) -> Result<(), Box<dyn Error>> {
    if queue.chocked {
        // return
    }
    while queue.length()>0 {
        let piece_block = queue.get_last();
        if pieces.needed(&piece_block) {
            stream.write(&msg_request(&piece_block)).await?;
            pieces.add_requested(&piece_block);
            break;
        }
    };

    Ok(())
}

fn msg_conn_req() -> [u8; 16] {
    let mut b: [u8; 16] = [0; 16];
    BigEndian::write_u64(&mut b[0..8], 0x41727101980); // connection_id
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[12..16].clone_from_slice(&random_bytes[..]);
    b
}

fn msg_ann_req(connection_id: u64, torrent: &Torrent, port: u16) -> Vec<u8> {
    let mut b: [u8; 98] = [0; 98];
    BigEndian::write_u64(&mut b[0..8], connection_id);
    BigEndian::write_u32(&mut b[8..12], ANNOUNCE_MSG); // action
    let random_bytes = rand::thread_rng().gen::<[u8; 4]>();
    b[12..16].clone_from_slice(&random_bytes[..]);
    b[16..36].clone_from_slice(&torrent.info_hash());
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

fn msg_resp_type(b: &Vec<u8>) -> u32 {
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
fn msg_parse_connect_resp(b: &Vec<u8>) -> ConnResp {
    ConnResp {
        action: BigEndian::read_u32(&b[0..4]),
        transaction_id: BigEndian::read_u32(&b[4..8]),
        connection_id: BigEndian::read_u64(&b[8..]),
    }
}

#[derive(Debug)]
struct Peer {
    ip: String,
    port: u16,
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
fn msg_parse_announce_resp(b: &Vec<u8>) -> AnnResp {
    let mut peers: Vec<Peer> = Vec::new();
    let n_peers = (b.len() - 20) / 6;
    for i in 0..n_peers {
        let peer: Peer = Peer {
            // ip: BigEndian::read_u32(&b[20+(6*i)..24+(6*i)]),
            ip: bytes_to_ip(
                &b[20 + (6 * i)..24 + (6 * i)]
                    .try_into()
                    .expect("err parsing peer ip"),
            ),
            port: BigEndian::read_u16(&b[24 + (6 * i)..26 + (6 * i)]),
        };
        peers.push(peer);
    }

    let ann_resp: AnnResp = AnnResp {
        action: BigEndian::read_u32(&b[0..4]),
        transaction_id: BigEndian::read_u32(&b[4..8]),
        interval: BigEndian::read_u32(&b[8..12]),
        leechers: BigEndian::read_u32(&b[12..16]),
        seeders: BigEndian::read_u32(&b[16..20]),
        peers: peers,
    };

    ann_resp
}

fn bytes_to_ip(b: &[u8; 4]) -> String {
    Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string()
}

fn msg_handshake(info_hash: Vec<u8>) -> Vec<u8> {
    let mut b: [u8; 68] = [0; 68];
    b[0] = 19;
    b[1..20].clone_from_slice(b"BitTorrent protocol"); // TODO use constant for the string

    b[28..48].clone_from_slice(&info_hash);

    b.to_vec()
}

fn msg_choke() -> Vec<u8> {
    let mut b: [u8; 5] = [0; 5];
    BigEndian::write_u32(&mut b[0..4], 1);
    b.to_vec()
}
fn msg_unchoke() -> Vec<u8> {
    let mut b: [u8; 5] = [0; 5];
    BigEndian::write_u32(&mut b[0..4], 1);
    b[4] = 1;
    b.to_vec()
}
fn msg_interested() -> Vec<u8> {
    let mut b: [u8; 5] = [0; 5];
    BigEndian::write_u32(&mut b[0..4], 1);
    b[4] = 2;
    b.to_vec()
}
fn msg_notinterested() -> Vec<u8> {
    let mut b: [u8; 5] = [0; 5];
    BigEndian::write_u32(&mut b[0..4], 1);
    b[4] = 3;
    b.to_vec()
}
fn msg_have(payload: Payload) -> Vec<u8> {
    let mut b: [u8; 9] = [0; 9];
    BigEndian::write_u32(&mut b[0..4], 5);
    b[4] = 4;
    BigEndian::write_u32(&mut b[5..9], payload.index); // TODO REVIEW CHECK payload.index
    b.to_vec()
}
fn msg_bitfield(bitfield: Vec<u8>) -> Vec<u8> {
    let mut b: [u8; 14] = [0; 14];
    BigEndian::write_u32(&mut b[0..4], bitfield.len() as u32);
    b[4] = 5;
    b[5..].clone_from_slice(&bitfield);
    b.to_vec()
}
fn msg_request(payload: &Payload) -> Vec<u8> {
    let mut b: [u8; 17] = [0; 17];
    BigEndian::write_u32(&mut b[0..4], 13);
    b[4] = 6;
    BigEndian::write_u32(&mut b[5..9], payload.index);
    BigEndian::write_u32(&mut b[9..13], payload.begin);
    BigEndian::write_u32(&mut b[13..17], payload.length);
    b.to_vec()
}
fn msg_piece(payload: Payload) -> Vec<u8> {
    let mut b: [u8; 13] = [0; 13]; // TODO payload.block.length
    BigEndian::write_u32(&mut b[0..4], payload.block.length+9);
    b[4] = 7;
    BigEndian::write_u32(&mut b[5..9], payload.index);
    BigEndian::write_u32(&mut b[9..13], payload.begin);
    b[13..].clone_from_slice(&payload.block.bytes);
    b.to_vec()
}
fn msg_cancel(payload: Payload) -> Vec<u8> {
    let mut b: [u8; 17] = [0; 17];
    BigEndian::write_u32(&mut b[0..4], 13);
    b[4] = 8;
    BigEndian::write_u32(&mut b[5..9], payload.index);
    BigEndian::write_u32(&mut b[9..13], payload.begin);
    BigEndian::write_u32(&mut b[13..17], payload.length);
    b.to_vec()
}
fn msg_port(port: u16) -> Vec<u8> {
    let mut b: [u8; 7] = [0; 7];
    BigEndian::write_u32(&mut b[0..4], 3);
    b[4] = 9;
    BigEndian::write_u16(&mut b[5..7], port);
    b.to_vec()
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
        assert_eq!(t.size(), 276_445_467);
    }

    #[test]
    fn msgs() {
        let t = Torrent::open("test.torrent");

        let conn_req = msg_conn_req().to_vec();
        assert_eq!(conn_req[0..12].to_hex(), "000004172710198000000000"); // [12..16] is random
        let conn_resp_buf = b"00000000d4c575c2c078f9a83418bebc";
        let conn_resp = msg_parse_connect_resp(&conn_resp_buf.to_vec());
        println!("conn_resp {:?}", conn_resp);
        assert_eq!(conn_resp.action, 808464432);
        assert_eq!(conn_resp.transaction_id, 808464432);
        assert_eq!(conn_resp.connection_id, 7220505182792409906);

        let announce_req = msg_ann_req(conn_resp.connection_id, &t, 6681);
        assert_eq!(announce_req[0..12].to_hex(), "643463353735633200000001");
        assert_eq!(announce_req[16..72].to_hex(), "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c0000000000000000000000000000000000000000000000000000000000000000107a391b");
        assert_eq!(announce_req[92..98].to_hex(), "ffffffff1a19");

        let announce_resp_buf = b"000000017a638a60000006e8000000030000002429d412a11ae12d53dcbd1a19d048c0e51ae1d8241960c8d5d5c36e71c8d5c5b974a31ae2c11ef348d4aabc5f370ec8d5bc4e16321ae1b9cee13b2327b94187b66f7cb94186b1c8d5b75308331ae1b068c0e0c263ac6f857911389d27ac821ae19088c2c01ae172fe2468c00a699aebe6720f68a2f99a1ae167fce2141ae163582ee41ae1634921a5d2725b72aaf5c8d559482e8fc9d257fb394cc8d55740e9ebd166524044e11aff51b6dcb6c8d54e2ebe61c3574b87860fc8d54b6e11026dff473afc7a1ec944cd47cd1ae131249b051ae124ff6b521ae105b75c13c8d50551153a1ae105024e09cd52";
        let ann_resp = msg_parse_announce_resp(&announce_resp_buf.to_vec());
        assert_eq!(ann_resp.peers.len(), 81);
        assert_eq!(ann_resp.peers[0].ip, "48.54.101.56");
        assert_eq!(ann_resp.peers[0].port, 12336);
        assert_eq!(ann_resp.peers[80].ip, "52.101.48.57");
        assert_eq!(ann_resp.peers[80].port, 25444);
    }

    #[test]
    fn pieces() {
        let t = Torrent::open("test.torrent");
        let mut p = Pieces::new(&t);
        // println!("{:?}", p);

        let payload: Payload = Payload{
            index: 0,
            begin: 0,
            length: 0,
            block: Block{
                length: 0,
                bytes: Vec::new()
            }
        };
        assert_eq!(true, p.needed(&payload));
        p.add_requested(&payload);
        p.add_received(&payload);
        assert_eq!(false, p.needed(&payload));
        assert_eq!(false, p.is_done());
    }
    
    #[async_std::test]
    async fn get_peers() {
        let t = Torrent::open("test.torrent");
        // println!("{:?}", t);
        println!("torrent: {:?}", t.info.name);
        println!("protocol: {:?}", t.protocol().unwrap());
    
        println!("msg_conn_req: {:?}", msg_conn_req());
    
        let peers_w = t.get_peers().await;
        println!("peers getted");
        let peers = peers_w.unwrap();
        // println!("get_peers peers: {:?}", peers);

    }
}
