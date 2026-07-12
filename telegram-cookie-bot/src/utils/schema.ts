import { z } from "zod"

export const CookieEntrySchema = z.object({
  name: z.string(),
  value: z.string(),
  domain: z.string(),
  path: z.string().optional(),
  secure: z.boolean().optional(),
  httpOnly: z.boolean().optional(),
  sameSite: z.string().optional(),
})

export const CookieArraySchema = z.array(CookieEntrySchema)

export type CookieEntry = z.infer<typeof CookieEntrySchema>
