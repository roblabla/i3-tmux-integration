error_chain! {
    types {
        Error, ErrorKind, ResultExt, Result;
    }

    foreign_links {
        I3Message(::i3ipc::MessageError);
        I3Establish(::i3ipc::EstablishError);
        Io(::std::io::Error);
        Nix(::nix::Error);
    }

}
