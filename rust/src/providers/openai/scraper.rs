//! OpenAI Dashboard Scraper
//!
//! JavaScript-based scraper for extracting usage data from the OpenAI/ChatGPT dashboard.
//! Uses React Fiber inspection to extract data from chart components.

use serde::{Deserialize, Serialize};

/// Usage breakdown by service (e.g., GPT-4, DALL-E)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBreakdown {
    /// Service name
    pub service: String,
    /// Hex color for the service in charts
    pub color: String,
    /// Usage amount in dollars
    pub amount: f64,
}

/// Credits usage history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditsHistoryEntry {
    /// Date string
    pub date: String,
    /// Description of usage
    pub description: String,
    /// Amount in dollars (positive = credit, negative = usage)
    pub amount: f64,
}

/// Scraped dashboard data
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIDashboardData {
    /// Remaining credits balance
    pub credits_remaining: Option<f64>,
    /// Total credits limit
    pub credits_limit: Option<f64>,
    /// Usage breakdown by service
    pub usage_breakdown: Vec<UsageBreakdown>,
    /// Credits usage history
    pub credits_history: Vec<CreditsHistoryEntry>,
    /// Account email
    pub email: Option<String>,
    /// Authentication status from ChatGPT bootstrap data
    pub auth_status: Option<String>,
    /// Account plan from ChatGPT bootstrap or Next data
    pub account_plan: Option<String>,
    /// Organization name
    pub organization: Option<String>,
    /// Purchase credits URL
    pub purchase_url: Option<String>,
}

impl OpenAIDashboardData {
    /// Calculate used percentage
    pub fn used_percent(&self) -> Option<f64> {
        let remaining = self.credits_remaining?;
        let limit = self.credits_limit?;
        if limit <= 0.0 {
            return None;
        }
        let used = limit - remaining;
        Some((used / limit) * 100.0)
    }

    /// Get total usage across all services
    pub fn total_usage(&self) -> f64 {
        self.usage_breakdown.iter().map(|b| b.amount).sum()
    }
}

/// JavaScript scrape script for OpenAI dashboard
///
/// This script is injected into the ChatGPT dashboard page to extract usage data.
/// It uses React Fiber inspection to access chart data that isn't directly in the DOM.
pub const OPENAI_DASHBOARD_SCRAPE_SCRIPT: &str = r#"
(() => {
  const textOf = el => {
    const raw = el && (el.innerText || el.textContent) ? String(el.innerText || el.textContent) : '';
    return raw.trim();
  };

  const parseHexColor = (color) => {
    if (!color) return null;
    const c = String(color).trim().toLowerCase();
    if (c.startsWith('#')) {
      if (c.length === 4) {
        return '#' + c[1] + c[1] + c[2] + c[2] + c[3] + c[3];
      }
      if (c.length === 7) return c;
      return c;
    }
    const m = c.match(/^rgba?\(([^)]+)\)$/);
    if (m) {
      const parts = m[1].split(',').map(x => parseFloat(x.trim())).filter(x => Number.isFinite(x));
      if (parts.length >= 3) {
        const r = Math.max(0, Math.min(255, Math.round(parts[0])));
        const g = Math.max(0, Math.min(255, Math.round(parts[1])));
        const b = Math.max(0, Math.min(255, Math.round(parts[2])));
        const toHex = n => n.toString(16).padStart(2, '0');
        return '#' + toHex(r) + toHex(g) + toHex(b);
      }
    }
    return c;
  };

  // React Fiber inspection for extracting chart data
  const reactPropsOf = (el) => {
    if (!el) return null;
    try {
      const keys = Object.keys(el);
      const propsKey = keys.find(k => k.startsWith('__reactProps$'));
      if (propsKey) return el[propsKey] || null;
      const fiberKey = keys.find(k => k.startsWith('__reactFiber$'));
      if (fiberKey) {
        const fiber = el[fiberKey];
        return (fiber && (fiber.memoizedProps || fiber.pendingProps)) || null;
      }
    } catch {}
    return null;
  };

  const reactFiberOf = (el) => {
    if (!el) return null;
    try {
      const keys = Object.keys(el);
      const fiberKey = keys.find(k => k.startsWith('__reactFiber$'));
      return fiberKey ? (el[fiberKey] || null) : null;
    } catch {
      return null;
    }
  };

  // Traverse React Fiber tree to find chart payload data
  const nestedBarMetaOf = (root) => {
    if (!root || typeof root !== 'object') return null;
    const queue = [root];
    const seen = typeof WeakSet !== 'undefined' ? new WeakSet() : null;
    let steps = 0;
    while (queue.length && steps < 250) {
      const cur = queue.shift();
      steps++;
      if (!cur || typeof cur !== 'object') continue;
      if (seen) {
        if (seen.has(cur)) continue;
        seen.add(cur);
      }
      if (cur.payload && (cur.dataKey || cur.name || cur.value !== undefined)) return cur;
      const values = Array.isArray(cur) ? cur : Object.values(cur);
      for (const v of values) {
        if (v && typeof v === 'object') queue.push(v);
      }
    }
    return null;
  };

  // Extract chart metadata from DOM element via React Fiber
  const barMetaFromElement = (el) => {
    const direct = reactPropsOf(el);
    if (direct && direct.payload && (direct.dataKey || direct.name || direct.value !== undefined)) return direct;

    const fiber = reactFiberOf(el);
    if (fiber) {
      let cur = fiber;
      for (let i = 0; i < 10 && cur; i++) {
        const props = (cur.memoizedProps || cur.pendingProps) || null;
        if (props && props.payload && (props.dataKey || props.name || props.value !== undefined)) return props;
        const nested = props ? nestedBarMetaOf(props) : null;
        if (nested) return nested;
        cur = cur.return || null;
      }
    }

    if (direct) {
      const nested = nestedBarMetaOf(direct);
      if (nested) return nested;
    }
    return null;
  };

  // Parse dollar amounts from text
  const parseDollarAmount = (text) => {
    if (!text) return null;
    const cleaned = String(text).replace(/[^0-9.,\-]/g, '');
    const num = parseFloat(cleaned.replace(',', ''));
    return Number.isFinite(num) ? num : null;
  };

  // Find credits remaining
  const findCreditsRemaining = () => {
    const patterns = [
      /\$?(\d+(?:\.\d+)?)\s*(?:credits?)?\s*(?:remaining|left|available)/i,
      /(?:remaining|left|available)[:\s]*\$?(\d+(?:\.\d+)?)/i,
      /balance[:\s]*\$?(\d+(?:\.\d+)?)/i,
    ];

    const textNodes = document.querySelectorAll('*');
    for (const node of textNodes) {
      const text = textOf(node);
      for (const pattern of patterns) {
        const match = text.match(pattern);
        if (match) {
          const num = parseFloat(match[1]);
          if (Number.isFinite(num)) return num;
        }
      }
    }
    return null;
  };

  // Find account email
  const findEmail = () => {
    const bootstrap = parseJsonScript('client-bootstrap');
    const bootstrapEmail = bootstrap?.session?.user?.email || bootstrap?.user?.email || null;
    if (bootstrapEmail && String(bootstrapEmail).includes('@')) return String(bootstrapEmail);

    const next = parseJsonScript('__NEXT_DATA__');
    const nextEmail = next?.props?.pageProps?.user?.email || next?.props?.session?.user?.email || null;
    if (nextEmail && String(nextEmail).includes('@')) return String(nextEmail);

    // Look for email patterns in the page
    const emailPattern = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/;
    const textNodes = document.querySelectorAll('[class*="email"], [class*="user"], [data-testid*="email"]');
    for (const node of textNodes) {
      const text = textOf(node);
      const match = text.match(emailPattern);
      if (match) return match[0];
    }
    return null;
  };

  const parseJsonScript = (id) => {
    try {
      const node = document.getElementById(id);
      const raw = node && node.textContent ? String(node.textContent) : '';
      return raw ? JSON.parse(raw) : null;
    } catch {
      return null;
    }
  };

  const cleanPlanName = (raw) => String(raw || '')
    .replace(/\b(claude|codex|account|plan)\b/gi, ' ')
    .replace(/_/g, ' ')
    .replace(/-/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

  const codexPlanDisplayName = (raw) => {
    const trimmed = String(raw || '').trim();
    if (!trimmed) return null;
    const exact = {
      pro: 'Pro 20x',
      prolite: 'Pro 5x',
      'pro_lite': 'Pro 5x',
      'pro-lite': 'Pro 5x',
      'pro lite': 'Pro 5x'
    };
    const lower = trimmed.toLowerCase();
    if (exact[lower]) return exact[lower];
    const cleaned = cleanPlanName(trimmed);
    if (!cleaned) return trimmed;
    const cleanedLower = cleaned.toLowerCase();
    if (exact[cleanedLower]) return exact[cleanedLower];
    return cleaned.split(' ')
      .filter(Boolean)
      .map(word => {
        const wordLower = word.toLowerCase();
        if (wordLower === 'cbp' || wordLower === 'k12') return wordLower.toUpperCase();
        if (word === word.toUpperCase() && /[a-z]/i.test(word)) return word;
        return word.charAt(0).toUpperCase() + word.slice(1);
      })
      .join(' ') || cleaned;
  };

  const normalizePlanValue = (value) => {
    const trimmed = String(value || '').trim();
    if (!trimmed) return null;
    const lower = trimmed.toLowerCase();
    const allowed = ['free', 'plus', 'pro', 'team', 'enterprise', 'business', 'edu', 'education', 'gov', 'premium', 'essential'];
    if (!allowed.some(token => lower.includes(token))) return null;
    return codexPlanDisplayName(trimmed) || cleanPlanName(trimmed);
  };

  const planCandidate = (key, value) => {
    const lower = String(key || '').toLowerCase();
    if (!lower.includes('plan') && !lower.includes('tier') && !lower.includes('subscription')) return null;
    if (typeof value === 'string') return normalizePlanValue(value);
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      return normalizePlanValue(value.name) || normalizePlanValue(value.displayName) || normalizePlanValue(value.tier);
    }
    return null;
  };

  const findPlan = (root) => {
    if (!root || typeof root !== 'object') return null;
    const queue = [root];
    const seenObjects = typeof WeakSet !== 'undefined' ? new WeakSet() : null;
    let index = 0;
    let seen = 0;
    while (index < queue.length && seen < 6000) {
      const cur = queue[index++];
      seen++;
      if (!cur || typeof cur !== 'object') continue;
      if (seenObjects) {
        if (seenObjects.has(cur)) continue;
        seenObjects.add(cur);
      }
      if (Array.isArray(cur)) {
        for (const value of cur) {
          if (value && typeof value === 'object') queue.push(value);
        }
        continue;
      }
      for (const [key, value] of Object.entries(cur)) {
        const plan = planCandidate(key, value);
        if (plan) return plan;
        if (value && typeof value === 'object') queue.push(value);
      }
    }
    return null;
  };

  // Extract usage breakdown from chart
  const extractUsageBreakdown = () => {
    const breakdown = [];

    // Find Recharts bar elements
    const bars = document.querySelectorAll('.recharts-bar-rectangle, [class*="bar"]');
    for (const bar of bars) {
      const meta = barMetaFromElement(bar);
      if (meta && meta.payload) {
        const name = meta.name || meta.dataKey || 'Unknown';
        const value = meta.value || meta.payload[meta.dataKey] || 0;
        const color = parseHexColor(bar.getAttribute('fill')) || '#888888';
        if (value > 0) {
          breakdown.push({ service: name, color, amount: value });
        }
      }
    }

    // Dedupe by service name
    const seen = new Set();
    return breakdown.filter(b => {
      if (seen.has(b.service)) return false;
      seen.add(b.service);
      return true;
    });
  };

  // Main scrape function
  const result = {
    credits_remaining: findCreditsRemaining(),
    credits_limit: null,
    usage_breakdown: extractUsageBreakdown(),
    credits_history: [],
    email: findEmail(),
    auth_status: (() => {
      const bootstrap = parseJsonScript('client-bootstrap');
      return typeof bootstrap?.authStatus === 'string' ? bootstrap.authStatus : null;
    })(),
    account_plan: findPlan(parseJsonScript('client-bootstrap')) || findPlan(parseJsonScript('__NEXT_DATA__')),
    organization: null,
    purchase_url: null
  };

  return JSON.stringify(result);
})();
"#;

/// Parse scraped JSON data into structured format
pub fn parse_dashboard_json(json: &str) -> Result<OpenAIDashboardData, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_breakdown() {
        let breakdown = UsageBreakdown {
            service: "GPT-4".to_string(),
            color: "#10a37f".to_string(),
            amount: 15.50,
        };

        assert_eq!(breakdown.service, "GPT-4");
        assert_eq!(breakdown.amount, 15.50);
    }

    #[test]
    fn test_dashboard_data_used_percent() {
        let data = OpenAIDashboardData {
            credits_remaining: Some(75.0),
            credits_limit: Some(100.0),
            ..Default::default()
        };

        assert_eq!(data.used_percent(), Some(25.0));
    }

    #[test]
    fn test_dashboard_data_total_usage() {
        let data = OpenAIDashboardData {
            usage_breakdown: vec![
                UsageBreakdown {
                    service: "GPT-4".to_string(),
                    color: "#10a37f".to_string(),
                    amount: 10.0,
                },
                UsageBreakdown {
                    service: "DALL-E".to_string(),
                    color: "#ff6b6b".to_string(),
                    amount: 5.0,
                },
            ],
            ..Default::default()
        };

        assert_eq!(data.total_usage(), 15.0);
    }

    #[test]
    fn test_parse_dashboard_json() {
        let json = r#"{"credits_remaining":50.0,"credits_limit":100.0,"usage_breakdown":[],"credits_history":[],"email":"test@example.com","auth_status":"logged_in","account_plan":"Pro 5x","organization":null,"purchase_url":null}"#;

        let data = parse_dashboard_json(json).unwrap();
        assert_eq!(data.credits_remaining, Some(50.0));
        assert_eq!(data.email, Some("test@example.com".to_string()));
        assert_eq!(data.auth_status, Some("logged_in".to_string()));
        assert_eq!(data.account_plan, Some("Pro 5x".to_string()));
    }
}
