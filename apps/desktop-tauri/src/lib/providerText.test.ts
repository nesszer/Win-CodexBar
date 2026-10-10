import { describe, expect, it } from "vitest";
import { fromFtl } from "../test/localeHarness";
import { localizeDates, localizeProviderText } from "./providerText";
import { localizeProviderLabel } from "./windowLabels";

const en = fromFtl("en-US.ftl");
const ru = fromFtl("ru-RU.ftl");

describe("localizeProviderText", () => {
  it.each([
    ["7 requests", "7 requests"],
    ["$12.40 API-rate", "$12.40 API-rate"],
    ["3/10 weekly credits", "3/10 weekly credits"],
    ["Offline · 2 conversations", "Offline · 2 conversations"],
    ["No Copilot quota reported", "No Copilot quota reported"],
    ["$1.20 / $5.00", "$1.20 / $5.00"],
    ["Claude 14d: 1 input, 2 output tokens", "Claude 14d: 1 input, 2 output tokens"],
    ["Resets in 2h 5m", "Resets in 2h 5m"],
    ["Renews in 2 months", "Renews in 2 mo"],
    ["120 of 500 left", "120 of 500 left"],
    ["Last 30 days (partial)", "Last 30 days (partial)"],
    ["Model: gpt-5 · Cost: $1.20", "Model: gpt-5 · Cost: $1.20"],
    ["expires Jan 5 at 3:00 PM", "expires Jan 5 at 3:00 PM"],
    ["Last 7 days · attributed", "Last 7 days · attributed"],
  ])("keeps English output unchanged for %j", (text, expected) => {
    expect(localizeProviderText(text, en)).toBe(expected);
  });

  it.each([
    ["7 requests", "Запросов: 7"],
    ["1,234 tokens", "Токенов: 1,234"],
    ["$12.40 API-rate", "$12.40 по тарифам API"],
    ["3/10 weekly credits", "Недельных кредитов: 3/10"],
    ["1.00 of 5.00 credits", "1.00 из 5.00 кредитов"],
    ["$4.10 over last 30 days", "$4.10 за последние 30 дн."],
    ["5 used, 3 remaining", "Использовано 5, осталось 3"],
    ["Offline · 2 conversations", "Офлайн · Диалогов: 2"],
    ["Offline · 1 conversation", "Офлайн · Диалогов: 1"],
    ["No balance information returned", "Данные о балансе не получены"],
    ["No Copilot quota reported", "Квота Copilot не сообщается"],
    ["No cached Windsurf quota details", "Нет сохраненных данных о квоте Windsurf"],
    ["Balance ¥3.00", "Баланс ¥3.00"],
    ["Hypercredit balance", "Баланс Hypercredit"],
    ["¥3.00 available", "Доступно: ¥3.00"],
    ["Not reported", "Не сообщается"],
    ["$2.00 · today", "$2.00 · за сегодня"],
    ["Resets in 2h 5m", "Сброс через 2 ч 5 мин"],
    ["Expires in 3d", "Истекает через 3 д"],
    ["Renews in 1 month", "Продление через 1 мес."],
    ["Cycle ends in 4h", "Цикл закончится через 4 ч"],
    ["120 of 500 left", "Осталось 120 из 500"],
    ["$1.00 of $5.00", "$1.00 из $5.00"],
    ["40/100 credits (60 remaining)", "Кредитов: 40/100 (осталось 60)"],
    ["Last 14 days", "Последние 14 дн."],
    ["Last 30 days (partial)", "Последние 30 дн. (частично)"],
    ["Last 7 days · attributed", "Последние 7 дн. · учтено"],
    ["Model: gpt-5 · Cost: $1.20", "Модель: gpt-5 · Стоимость: $1.20"],
    ["5 credits expire on Jan 5", "5 кредитов сгорят 5 янв."],
    ["No OpenAI API balance available", "Нет баланса API OpenAI"],
  ])("translates %j", (text, expected) => {
    expect(localizeProviderText(text, ru)).toBe(expected);
  });

  it("passes unknown text and bare numbers through", () => {
    expect(localizeProviderText("$1.20 / $5.00", ru)).toBe("$1.20 / $5.00");
    expect(localizeProviderText("Gemini Pro", ru)).toBe("Gemini Pro");
    expect(localizeProviderText(null, ru)).toBe("");
  });
});

describe("localizeProviderLabel", () => {
  it("keeps brands and localizes generic tails", () => {
    expect(localizeProviderText("Daily spend", ru)).toBe(ru("ProviderLabelDailySpend"));
    expect(localizeProviderLabel("Monthly", ru)).toBe(ru("ProviderLabelMonthly"));
    expect(localizeProviderLabel("Gemini Weekly", ru)).toBe(`Gemini ${ru("ProviderWeeklyLabel")}`);
    expect(localizeProviderLabel("Codex Spark 5-hour", ru)).toBe(
      `Codex Spark ${ru("WindowLabelHours").replace("{}", "5")}`,
    );
    expect(localizeProviderLabel("Opus only", ru)).toBe(ru("ProviderLabelModelOnly").replace("{}", "Opus"));
    expect(localizeProviderLabel("Team Acme", ru)).toBe(ru("ProviderLabelTeamNamed").replace("{}", "Acme"));
    expect(localizeProviderLabel("5 hour limit", ru)).toBe(ru("ProviderLabelHourLimit").replace("{}", "5"));
    expect(localizeProviderLabel("Gemini Pro", ru)).toBe("Gemini Pro");
    expect(localizeProviderLabel("Gemini Weekly", en)).toBe("Gemini Weekly");
  });
});

describe("localizeDates", () => {
  it("reformats English dates in the UI locale and leaves English alone", () => {
    expect(localizeDates("Jan 5 at 3:00 PM UTC", en)).toBe("Jan 5 at 3:00 PM UTC");
    expect(localizeDates("Feb 29", ru)).toBe("29 февр.");
    expect(localizeDates("Jan 5 at 3:00 PM UTC", ru)).toBe("5 янв., 15:00 UTC");
    expect(localizeDates("Mar 3, 2026", ru)).toMatch(/^3 мар\. 2026/);
    expect(localizeDates("Feb 30", ru)).toBe("Feb 30");
  });
});
