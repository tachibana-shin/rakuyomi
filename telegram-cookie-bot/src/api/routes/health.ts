import { OpenAPIHono } from "@hono/zod-openapi"
import { createRoute, z } from "@hono/zod-openapi"

const healthRoute = createRoute({
  method: "get",
  path: "/health",
  tags: ["Health"],
  responses: {
    200: {
      content: { "text/plain": { schema: z.literal("OK") } },
      description: "Health check",
    },
  },
})

const app = new OpenAPIHono()

app.openapi(healthRoute, (c) => c.text("OK"))

export default app
