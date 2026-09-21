p = "C:/Users/mac/Documents/Codes/wcb-580/rust/src/providers/muse/local_usage/cache.rs"
s = open(p, encoding="utf-8").read()
s = s.replace("pub(crate) identity:: String,", "pub(crate) identity: String,")
s = s.replace("pub(crate) version:: u32,", "pub(crate) version: u32,")
s = re.sub(r"pub\(crate\) ([a-z_]+):: ", r"pub(crate) \1: ", s)
open(p, "w", encoding="utf-8", newline="").write(s)
print("colon typos fixed")
