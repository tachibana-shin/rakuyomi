import type { FC } from "hono/jsx"

export const ResultPage: FC<{ title: string; message: string; ok: boolean }> = ({
  title,
  message,
  ok,
}) => {
  const color = ok ? "#4caf50" : "#e74c3c"
  const icon = ok ? "\u2713" : "\u2717"

  return (
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>{title}</title>
        <style
          dangerouslySetInnerHTML={{
            __html: `
              * { margin: 0; padding: 0; box-sizing: border-box; }
              body {
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                background: #1a1a2e; color: #e0e0e0;
                display: flex; align-items: center; justify-content: center;
                min-height: 100vh; padding: 20px;
              }
              .card {
                background: #16162a; border-radius: 16px; padding: 40px 32px;
                max-width: 400px; text-align: center;
              }
              .icon { font-size: 48px; margin-bottom: 16px; }
              h1 { font-size: 18px; font-weight: 700; margin-bottom: 8px; color: ${color}; }
              p { font-size: 14px; color: #888; line-height: 1.5; }
            `,
          }}
        />
      </head>
      <body>
        <div class="card">
          <div class="icon">{icon}</div>
          <h1>{title}</h1>
          <p>{message}</p>
        </div>
      </body>
    </html>
  )
}
