p = "C:/Users/mac/Documents/Codes/wcb-574/rust/src/providers/antigravity/mod.rs"
lines = open(p, encoding="utf-8").read().splitlines(keepends=True)

# Conflict: lines 737 (marker) through 768 (origin/main closing brace region).
# HEAD arm (738-749-ish) lacks CSRF gate + deeper nesting; theirs has CSRF gate
# with string "cli". Union: take theirs structure, swap "cli" for enum call.
head_arm = []
theirs_arm = []
mode = None
start = None
end = None
for i, l in enumerate(lines):
    if l.startswith("<<<<<<< HEAD"):
        start = i
        mode = "head"
        continue
    if l.rstrip() == "=======" and mode == "head":
        mode = "theirs"
        continue
    if l.startswith(">>>>>>> origin/main"):
        end = i
        break
    if mode == "head":
        head_arm.append(l)
    elif mode == "theirs":
        theirs_arm.append(l)

merged = []
for l in theirs_arm:
    merged.append(l.replace('result.source_label = "cli".to_string();',
                            "result.source_label = AntigravityStrategyId::Cli.as_str().to_string();"))
# theirs arm ends without the final closing braces that follow the >>>>>>> marker;
# the two lines after the marker (line 768-769: "}" "}") belong to the else{} close
after = lines[end + 1:end + 3]
merged_block = merged
new_lines = lines[:start] + merged_block + after + lines[end + 3:]
open(p, "w", encoding="utf-8", newline="").write("".join(new_lines))
print("resolved; markers left:", "".join(new_lines).count("<<<<<<<"))
