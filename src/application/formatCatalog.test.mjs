import test from "node:test";
import assert from "node:assert/strict";
import {
  availableOutputFormats, filterOutputFormats, findFormatByExtension, pickSupportedFormat
} from "./formatCatalog.mjs";

const capabilities = {
  families: [
    {
      family: "image",
      descriptors: [
        { id: "png", canonical_extension: "png", aliases: [], can_read: true, can_write: true },
        { id: "jpg", canonical_extension: "jpg", aliases: ["jpeg"], can_read: true, can_write: true },
        { id: "qoi", canonical_extension: "qoi", aliases: [], can_read: true, can_write: true },
        { id: "pdf", canonical_extension: "pdf", aliases: [], can_read: false, can_write: true },
      ],
      routes: [
        { input: "png", outputs: ["jpg", "qoi", "pdf"] },
        { input: "jpg", outputs: ["png", "qoi"] },
        { input: "qoi", outputs: ["png", "jpg", "qoi"] },
      ],
    },
    {
      family: "audio",
      descriptors: [
        { id: "wav", canonical_extension: "wav", aliases: [], can_read: true, can_write: true },
      ],
      routes: [{ input: "wav", outputs: ["wav"] }],
    },
    {
      family: "video",
      descriptors: [
        { id: "mp4", canonical_extension: "mp4", aliases: ["m4v"], can_read: true, can_write: true },
        { id: "mkv", canonical_extension: "mkv", aliases: [], can_read: true, can_write: true },
        { id: "gif", canonical_extension: "gif", aliases: [], can_read: false, can_write: true },
        { id: "wav", canonical_extension: "wav", aliases: [], can_read: false, can_write: true },
      ],
      routes: [
        { input: "mp4", outputs: ["mkv", "gif", "wav"] },
        { input: "mkv", outputs: ["mp4", "gif", "wav"] },
      ],
    },
  ],
};

test("QOI appears from capabilities without an explicit UI format constant", () => {
  assert.deepEqual(availableOutputFormats(capabilities, "image", ["png"]), ["jpg", "qoi", "pdf"]);
});
test("multiple input formats require intersection, never union", () => {
  assert.deepEqual(availableOutputFormats(capabilities, "image", ["png", "jpeg"]), ["qoi"]);
});
test("unrecognized input fails closed", () => {
  assert.deepEqual(availableOutputFormats(capabilities, "image", ["unknown"]), []);
});
test("new format count scales independently from UI code", () => {
  const ids = Array.from({length: 50}, (_, i) => "fmt" + i);
  const family = {
    family: "extended",
    descriptors: ids.map(id => ({id, canonical_extension:id, aliases:[], can_read:true, can_write:true})),
    routes: [{input:"fmt0", outputs:ids}],
  };
  const caps = {families:[family]};
  assert.equal(availableOutputFormats(caps, "extended", ["fmt0"]).length, 50);
  assert.deepEqual(filterOutputFormats(ids, "fmt4"), ids.filter(id => id.includes("fmt4")));
});
test("alias lookup is case insensitive and respects can_read", () => {
  assert.equal(findFormatByExtension(capabilities, ".JPEG")?.descriptor.id, "jpg");
  assert.equal(findFormatByExtension(capabilities, "PDF"), null);
});
test("saved unsupported preferences fall back to an advertised format", () => {
  assert.equal(pickSupportedFormat("mp3", ["jpg","qoi"]), "jpg");
  assert.equal(pickSupportedFormat("qoi", ["jpg","qoi"]), "qoi");
  assert.equal(pickSupportedFormat("old", []), "");
});
test("no advertised family returns no output formats", () => {
  assert.deepEqual(availableOutputFormats(null, "image"), []);
  assert.deepEqual(availableOutputFormats(capabilities, "missing"), []);
});

test("video family reads its containers and offers cross-family outputs", () => {
  assert.equal(findFormatByExtension(capabilities, "m4v").family, "video");
  assert.deepEqual(availableOutputFormats(capabilities, "video", ["mp4"]), ["mkv", "gif", "wav"]);
  assert.deepEqual(availableOutputFormats(capabilities, "video", ["mp4", "mkv"]), ["gif", "wav"]);
});
test("write-only names owned by other families still resolve to their reader", () => {
  assert.equal(findFormatByExtension(capabilities, "gif")?.family, undefined);
  assert.equal(findFormatByExtension(capabilities, "wav").family, "audio");
});
