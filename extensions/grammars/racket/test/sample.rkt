#lang racket/base
;; A small sample touching most of the language.
(require racket/list
         racket/string)

(provide greet
         (struct-out point))

#| A block comment
   spanning two lines |#

(struct point (x y) #:transparent)
(define-struct shape (name sides))

(define pi-ish 3.14159)
(define big 1e10)
(define hex #xFF)
(define ratio 1/3)

(define (greet name)
  (string-append "Hello, " name "!\n"))

(define (distance p q)
  (define dx (- (point-x p) (point-x q)))
  (define dy (- (point-y p) (point-y q)))
  (sqrt (+ (* dx dx) (* dy dy))))

(define (fact n)
  (if (<= n 1)
      1
      (* n (fact (- n 1)))))

(define square (lambda (x) (* x x)))

#;(this-is-a-datum-comment (ignored))

(define (classify n)
  (cond
    [(< n 0) 'negative]
    [(= n 0) 'zero]
    [else 'positive]))

(define (describe v)
  (match v
    [(point x y) (format "point ~a ~a" x y)]
    [_ "unknown"]))

(let loop ([i 0] [acc '()])
  (when (< i 5)
    (loop (add1 i) (cons (square i) acc))))

(for ([x (in-range 3)]
      #:when (even? x))
  (printf "x = ~a\n" x))

(define table (hash 'a 1 'b 2 #:extra #t))
(define chars (list #\a #\space #\newline))
(define pattern #rx"[a-z]+")
(define bytes-val #"bytes\x00")
(define flag (and #t #f))

(module+ main
  (displayln (greet "world"))
  (displayln (distance (point 0 0) (point 3 4)))
  (displayln (map fact (range 1 6))))
