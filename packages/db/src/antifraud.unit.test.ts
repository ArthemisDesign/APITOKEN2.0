import { describe, expect, it } from "vitest";
import { canonicalizeEmail, ipSubnetOf, isBonusEligibleEmailDomain } from "./antifraud.js";

describe("isBonusEligibleEmailDomain", () => {
  it("allows popular providers", () => {
    expect(isBonusEligibleEmailDomain("user@gmail.com")).toBe(true);
    expect(isBonusEligibleEmailDomain("user@GoogleMail.com")).toBe(true);
    expect(isBonusEligibleEmailDomain("user@yandex.ru")).toBe(true);
    expect(isBonusEligibleEmailDomain("user@icloud.com")).toBe(true);
    expect(isBonusEligibleEmailDomain("user@naver.com")).toBe(true);
  });

  it("denies providers without mandatory SMS verification", () => {
    expect(isBonusEligibleEmailDomain("user@proton.me")).toBe(false);
    expect(isBonusEligibleEmailDomain("user@protonmail.com")).toBe(false);
    expect(isBonusEligibleEmailDomain("user@gmx.de")).toBe(false);
  });

  it("denies Microsoft family and Chinese freemail", () => {
    expect(isBonusEligibleEmailDomain("helddon@outlook.com")).toBe(false);
    expect(isBonusEligibleEmailDomain("user@hotmail.com")).toBe(false);
    expect(isBonusEligibleEmailDomain("108608598@qq.com")).toBe(false);
    expect(isBonusEligibleEmailDomain("user@163.com")).toBe(false);
  });

  it("denies temp and custom domains", () => {
    expect(isBonusEligibleEmailDomain("mrwav9t6e75i@animatimg.com")).toBe(false);
    expect(isBonusEligibleEmailDomain("ffz@mail.nodeloc.cc")).toBe(false);
  });
});

describe("canonicalizeEmail", () => {
  it("collapses gmail dots and plus aliases", () => {
    expect(canonicalizeEmail("Lin.Astc+promo@GMail.com")).toBe("linastc@gmail.com");
    expect(canonicalizeEmail("l.i.n.astc666@gmail.com")).toBe("linastc666@gmail.com");
  });

  it("maps googlemail to gmail", () => {
    expect(canonicalizeEmail("user.name@googlemail.com")).toBe("username@gmail.com");
  });

  it("strips only plus aliases for other domains, keeping dots", () => {
    expect(canonicalizeEmail("hill.mark+zf8uk@outlook.com")).toBe("hill.mark@outlook.com");
  });

  it("lowercases and trims", () => {
    expect(canonicalizeEmail("  USER@Example.COM ")).toBe("user@example.com");
  });
});

describe("ipSubnetOf", () => {
  it("maps IPv4 to /24", () => {
    expect(ipSubnetOf("203.0.113.77")).toBe("203.0.113.0/24");
  });

  it("maps IPv6 to /64 with :: expansion", () => {
    expect(ipSubnetOf("2001:db8:aaaa:bbbb:1:2:3:4")).toBe("2001:db8:aaaa:bbbb::/64");
    expect(ipSubnetOf("2001:db8::1")).toBe("2001:db8:0:0::/64");
  });

  it("unwraps IPv4-mapped IPv6", () => {
    expect(ipSubnetOf("::ffff:203.0.113.77")).toBe("203.0.113.0/24");
  });

  it("returns null for garbage and missing input", () => {
    expect(ipSubnetOf(null)).toBeNull();
    expect(ipSubnetOf("not-an-ip")).toBeNull();
    expect(ipSubnetOf("1:2:3::4::5")).toBeNull();
  });
});
