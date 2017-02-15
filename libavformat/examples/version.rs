extern crate libavformat;
use libavformat::avformat_version;

fn main(){
    println!("avformat version : {}",unsafe {avformat_version()});
}
