use std;
use nom;
use nom::IResult::*;

#[derive(Debug)]
pub enum Container {
    Pane(u64),
    LeftRightLayout(Vec<Layout>),
    TopDownLayout(Vec<Layout>)
}

#[derive(Debug)]
pub struct Layout {
    width: u64,
    height: u64,
    xoff: u64,
    yoff: u64,
    contains: Container
}

impl Layout {
    pub fn parse(layout_bytes: &str) -> Result<Layout, nom::Err<&str>> {
        match layout(layout_bytes) {
            Done(rest, lay) => Ok(lay),
            Error(err) => Err(err),
            Incomplete(_) => unreachable!() // TODO
        }
    }
}

named!(u64_digit<&str, u64>,
    map_res!(
        nom::digit,
        std::str::FromStr::from_str
    )
);

named!(container<&str, Container>,
    alt!(
        chain!(
            tag_s!("{") ~
            layout1: layout ~
            mut layout_rest: many1!(chain!(
                tag_s!(",") ~
                layout: layout,
                || { layout })) ~
            tag_s!("}"),
            ||{layout_rest.push(layout1); Container::LeftRightLayout(layout_rest)}
        ) => { |layout| layout }
      | chain!(
            tag_s!("[") ~
            layout1: layout ~
            mut layout_rest: many1!(chain!(
                tag_s!(",") ~
                layout: layout,
                || { layout })) ~
            tag_s!("]"),
            ||{layout_rest.push(layout1); Container::TopDownLayout(layout_rest)}
        ) => { |layout| layout }
      | chain!(
          tag_s!(",") ~
          paneid: u64_digit,
          || Container::Pane(paneid)
        ) => { |layout| layout }
    )
);

named!(layout<&str, Layout>,
    chain!(
        width: u64_digit ~
        tag_s!("x") ~
        height: u64_digit ~
        tag_s!(",") ~
        xoff: u64_digit ~
        tag_s!(",") ~
        yoff: u64_digit ~
        container: container,
        || { Layout {
            width: width,
            height: height,
            xoff: xoff,
            yoff: yoff,
            contains: container
        }}
    )
);

