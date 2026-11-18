import { describe, expect, it, vi } from "vitest";
import { mockApi } from "./mock";

// The browser mock stands in for the Tauri shell's commands. Where the Rust side refuses
// something, the mock must refuse it too, or the UI's error paths never run in tests.
describe("mock shell commands refuse what Rust refuses", () => {
  it("previews only loopback http with page tools", async () => {
    const ok = await mockApi.previewProxyStart("a1", "http://localhost:5173");
    expect(ok.simulated).toBe(true);
    await expect(mockApi.previewProxyStart("a1", "http://127.0.0.1:3000")).resolves.toBeTruthy();
    await expect(mockApi.previewProxyStart("a1", "http://[::1]:3000")).resolves.toBeTruthy();
    await expect(mockApi.previewProxyStart("a1", "http://192.168.1.5:3000")).rejects.toThrow(/localhost/);
    await expect(mockApi.previewProxyStart("a1", "https://localhost:3000")).rejects.toThrow(/http:\/\//);
  });

  it("opens web links and absolute paths, nothing else", async () => {
    const open = vi.fn(() => null);
    vi.stubGlobal("window", { open });
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    try {
      await mockApi.openPath("https://example.com/a");
      expect(open).toHaveBeenCalledTimes(1);
      await expect(mockApi.openPath("/home/sam/project")).resolves.toBeUndefined();
      await expect(mockApi.openPath("C:\\Users\\sam")).resolves.toBeUndefined();
      await expect(mockApi.openPath("mailto:a@b.c")).resolves.toBeUndefined();
      await expect(mockApi.openPath("file:///etc/passwd")).rejects.toThrow(/Links of type file/);
      await expect(mockApi.openPath("javascript:alert(1)")).rejects.toThrow(/javascript/);
      await expect(mockApi.openPath("src/main.ts")).rejects.toThrow(/absolute path/);
      expect(open).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
      info.mockRestore();
    }
  });

  it("gives share links only the lifetimes Rust allows", async () => {
    const settings = await mockApi.getState().then((s) => s.settings);
    const agentId = (await mockApi.getState()).agents[0].id;
    await mockApi.updateSettings({ ...settings, remote: { ...settings.remote, enabled: false } });
    await expect(mockApi.createShareLink(agentId, 24)).rejects.toThrow(/Turn on remote access/);
    await mockApi.updateSettings({ ...settings, remote: { ...settings.remote, enabled: true } });
    const week = await mockApi.createShareLink(agentId, 168);
    expect(week.expiresAt - week.createdAt).toBe(168 * 3_600_000);
    const odd = await mockApi.createShareLink(agentId, 5);
    expect(odd.expiresAt - odd.createdAt).toBe(24 * 3_600_000);
    await mockApi.updateSettings(settings);
  });
});
