// Which route responses carry a live body (§7.15, §20). The engine maps
// `stream<list<u8>>` to `application/octet-stream` and `stream<record>` to
// `text/event-stream`; every other response is buffered as before.

export function isStreamingContentType(
  contentType: string | null | undefined,
): boolean {
  if (!contentType) return false;
  const type = contentType.split(";", 1)[0].trim().toLowerCase();
  return type === "application/octet-stream" || type === "text/event-stream";
}
