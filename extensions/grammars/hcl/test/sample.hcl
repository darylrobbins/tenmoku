# Service definition, in plain HCL.
/* A block comment
   over two lines. */
// A line comment in the other style.

locals {
  name        = "web-${var.environment}"
  port        = 8080
  ratio       = 0.75
  big         = 1.5e3
  enabled     = true
  nothing     = null
  tags = {
    Team  = "platform"
    Owner = "ops"
    "cost-center" = 1234
  }
  zones = ["a", "b", "c"]
}

service "http" "frontend" {
  listen = "0.0.0.0:${local.port}"
  path   = "C:\\temp\\\"quoted\"\n"
  count  = var.enabled ? 3 : 0

  health_check {
    interval = "10s"
    timeout  = "2s"
    retries  = max(3, length(local.zones))
  }

  upstream "primary" {
    host   = upper(local.name)
    weight = -1 * (2 + 3) % 4
  }

  env = { for k, v in local.tags : upper(k) => v if v != "" }
  ids = [for z in local.zones : "${z}-${count.index}"]
  all = local.zones[*]
  first = local.zones[0]
  merged = merge(local.tags, { Extra = "yes" })
  check = !local.enabled || (local.port >= 80 && local.port <= 9000)
  args  = concat(["--verbose"], local.zones...)
}

banner = <<EOT
Welcome to ${local.name}
%{ if local.enabled ~}
Service is enabled.
%{ else ~}
Service is disabled.
%{ endif ~}
%{ for z in local.zones }
zone ${z}
%{ endfor }
EOT

indented = <<-TEXT
  trimmed
    keeps relative indent
  TEXT
