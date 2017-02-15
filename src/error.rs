use error_chain;

error_chain!{
    foreign_links {
        DLOpenError(::x11_dl::error::OpenError);
    }
}
