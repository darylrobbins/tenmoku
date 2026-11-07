#!/usr/bin/env Rscript
# Summarise a data set and fit a simple model.

library(stats)
suppressPackageStartupMessages(library(utils))

MAX_ROWS <- 1000L
tolerance = 1e-6
ratio <- 0.75
z <- 3+2i
hex <- 0xFF

#' Compute the mean of the positive values.
#' @param x A numeric vector.
positive_mean <- function(x, na.rm = TRUE) {
  x <- x[x > 0]
  if (length(x) == 0) {
    return(NA)
  } else if (anyNA(x) && !na.rm) {
    stop("missing values: \"x\"\n")
  }
  sum(x, na.rm = na.rm) / length(x)
}

square <- \(x) x^2

describe <- function(df, ...) {
  out <- list()
  for (name in names(df)) {
    col <- df[[name]]
    if (is.numeric(col)) {
      out[[name]] <- c(mean = mean(col), sd = sd(col))
    } else {
      next
    }
  }
  i <- 0
  while (TRUE) {
    i <- i + 1
    if (i >= 10) break
  }
  repeat {
    break
  }
  out
}

data <- data.frame(
  id = seq_len(10),
  value = rnorm(10, mean = 5, sd = 2),
  label = rep(c("a", "b"), times = 5)
)

result <- data |>
  subset(value > 0) |>
  head(n = 5)

fit <- lm(value ~ id, data = data)
coefs <- stats::coef(fit)
print(summary(fit)$r.squared)

message('Fitted ', nrow(data), " rows\t", Inf, " ", NULL)
x <- c(TRUE, FALSE, NA, NaN)
for (j in seq(1, 3)) cat(j, "\n")
sapply(1:3, function(k) k %in% c(1, 3))
5 -> w
