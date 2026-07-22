import { describe, expect, it } from "vitest";
import { canonicalizeEmail, ipSubnetOf } from "./antifraud.js";

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
