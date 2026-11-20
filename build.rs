//! Embeds the version details (description, product, company, copyright and the package version) in the Windows
//! exes, where Explorer shows them under Properties > Details.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set("FileDescription", "OP2Imager");
        res.set("ProductName", "OP2Imager");
        res.set("CompanyName", "Outpost Universe");
        res.set("LegalCopyright", "Outpost Universe © 2026");
        if let Err(e) = res.compile() {
            println!("cargo:warning=version details not embedded: {e}");
        }
    }
}
