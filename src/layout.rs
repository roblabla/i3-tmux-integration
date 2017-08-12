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
    pub fn parse(layout_bytes: &str) -> Result<Layout, nom::Err> {
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
        delimited!(tag_s!("{"), separated_nonempty_list_complete!(tag_s!(","), layout), tag_s!("}")) => { |l| Container::LeftRightLayout(l) } |
        delimited!(tag_s!("["), separated_nonempty_list_complete!(tag_s!(","), layout), tag_s!("]")) => { |l| Container::TopDownLayout(l) } |
        preceded!(tag_s!(","), u64_digit) => { |paneid| Container::Pane(paneid) }
    )
);

named!(layout<&str, Layout>,
    do_parse!(
        width: u64_digit >>
        tag_s!("x") >>
        height: u64_digit >>
        tag_s!(",") >>
        xoff: u64_digit >>
        tag_s!(",") >>
        yoff: u64_digit >>
        container: container >>
        (Layout {
            width: width,
            height: height,
            xoff: xoff,
            yoff: yoff,
            contains: container
        })
    )
);

