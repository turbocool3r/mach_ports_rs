use crate::{msg::*, rights::*};

macro_rules! check_msg {
    ($parser:ident $(desc $desc:ident,)* body $body:expr) => {
        {
            let parser = $parser;

            $(
                let parser = match parser {
                    $crate::msg::DescOrBodyParser::Descriptor(desc) => {
                        let (desc, parser) = desc.next();

                        assert!(matches!(desc, $crate::msg::ParsedMsgDesc::$desc(_)));

                        parser
                    }
                    $crate::msg::DescOrBodyParser::Body(_) => {
                        panic!("expected one more descriptor");
                    }
                };
            )+

            match parser {
                $crate::msg::DescOrBodyParser::Descriptor(_) => {
                    panic!("expected a body parser");
                }
                $crate::msg::DescOrBodyParser::Body(parser) => {
                    assert_eq!(parser.body(), $body);
                }
            }
        }
    }
}

#[test]
fn test_send_recv() {
    let right = RecvRight::alloc();
    let send_right = right.make_send();

    let mut buffer = Buffer::with_capacity(4096);
    let mut builder = Builder::new(&mut buffer);
    builder.append_inline_data(b"test");
    builder.append_copied_send_right(&send_right);
    builder.append_made_send_right(&right, true);
    send_right.send(builder).unwrap();

    let parser = right.recv(&mut buffer).unwrap();
    let (_header, parser) = parser.parse_header();

    check_msg!(parser desc PortSend, desc PortSendOnce, body b"test");
}

#[test]
fn test_send_consumed_recv() {
    let right = RecvRight::alloc();
    let send_right = right.make_send();

    let mut buffer = Buffer::with_capacity(4096);
    let mut builder = Builder::new(&mut buffer);
    builder.append_moved_right(right);
    send_right.send(builder).unwrap();
}
