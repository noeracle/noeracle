// Minimal Server-Sent-Events reader over fetch — works in Node 18+ and
// browsers, with no EventSource dependency. Reconnects automatically with a
// short backoff until the abort signal fires.

export interface SseHandlers {
  onEvent: (event: string, data: string) => void;
  onError: (err: Error) => void;
}

export function streamSse(
  url: string,
  handlers: SseHandlers,
  signal: AbortSignal,
): void {
  const run = async (): Promise<void> => {
    while (!signal.aborted) {
      try {
        const res = await fetch(url, { signal });
        if (!res.ok || !res.body) {
          throw new Error(`stream returned HTTP ${res.status}`);
        }
        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        while (!signal.aborted) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          let sep: number;
          while ((sep = buffer.indexOf("\n\n")) >= 0) {
            const frame = buffer.slice(0, sep);
            buffer = buffer.slice(sep + 2);
            let event = "message";
            const data: string[] = [];
            for (const line of frame.split("\n")) {
              if (line.startsWith("event:")) event = line.slice(6).trim();
              else if (line.startsWith("data:")) data.push(line.slice(5).trim());
            }
            if (data.length > 0) handlers.onEvent(event, data.join("\n"));
          }
        }
      } catch (err) {
        if (signal.aborted) return;
        handlers.onError(err as Error);
      }
      if (!signal.aborted) {
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
  };
  void run();
}
