# Terraform sample
terraform {
  required_version = ">= 1.5.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

/* Inputs
   and locals */
variable "region" {
  type        = string
  default     = "us-east-1"
  description = "Region with a \"quoted\" word\n"
}

variable "instance_count" {
  type    = number
  default = 3
}

variable "enabled" {
  type    = bool
  default = true
}

locals {
  prefix = "app-${var.region}"
  ratio  = 0.75
  tags = {
    Name        = "${local.prefix}-web"
    Environment = var.enabled ? "prod" : "dev"
  }
  names = [for i in range(var.instance_count) : "web-${i}"]
  upper = { for k, v in local.tags : k => upper(v) if v != null }
}

provider "aws" {
  region = var.region
}

resource "aws_instance" "web" {
  count         = var.enabled ? var.instance_count : 0
  ami           = data.aws_ami.ubuntu.id
  instance_type = "t3.micro"

  tags = merge(local.tags, { Index = count.index })

  lifecycle {
    create_before_destroy = true
  }
}

data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"]
}

resource "aws_iam_policy" "p" {
  name   = "policy"
  policy = <<-EOT
    {
      "Name": "${local.prefix}",
      %{ if var.enabled }"on": true,%{ endif }
      "ids": [${join(",", aws_instance.web[*].id)}]
    }
  EOT
}

output "ids" {
  value = aws_instance.web[*].id
}
