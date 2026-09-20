p = "C:/Users/mac/Documents/Codes/wcb-572/apps/desktop-tauri/src-tauri/src/commands/bridge.rs"
s = open(p, encoding="utf-8").read()
dup = """            inventory: result
                .inventory
                .iter()
                .map(|item| ProviderInventoryItemSnapshot {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    available_count: item.available_count,
                    next_expires_at: item.next_expires_at.map(|date| date.to_rfc3339()),
                })
                .collect(),
"""
first = s.find(dup)
second = s.find(dup, first + 1)
s = s[:second] + s[second + len(dup):]
open(p, "w", encoding="utf-8", newline="").write(s)
print("removed dup; count now", s.count("inventory: result"))
