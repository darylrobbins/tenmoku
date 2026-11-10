;;; Sample Scheme program: lists, records, macros and strings.

#| A block comment
   spanning two lines |#

(import (scheme base)
        (scheme write))

(define-record-type point
  (make-point x y)
  point?
  (x point-x)
  (y point-y set-point-y!))

(define pi 3.14159)
(define answer 42)
(define big #xFF)
(define ratio 1/3)
(define greeting "Hello, \"world\"!\n")
(define letter #\a)
(define flag #t)

;; Recursive factorial
(define (factorial n)
  (if (<= n 1)
      1
      (* n (factorial (- n 1)))))

(define (fib n)
  (let loop ((a 0) (b 1) (i 0))
    (cond ((= i n) a)
          (else (loop b (+ a b) (+ i 1))))))

(define-syntax swap!
  (syntax-rules ()
    ((_ a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(define (distance p q)
  (let ((dx (- (point-x p) (point-x q)))
        (dy (- (point-y p) (point-y q))))
    (sqrt (+ (* dx dx) (* dy dy)))))

(define (process items)
  (map (lambda (x) (* x x))
       (filter odd? items)))

#;(this-is-ignored "x" (nested (deeper 1 2) (define y 3)))

(define primes '(2 3 5 7 11))
(define quoted '(a (b (c d))))
(define vec #(1 2 3))

(let* ((p (make-point 3 4))
       (q (make-point 0 0)))
  (display "distance: ")
  (display (distance p q))
  (newline))

(for-each
 (lambda (n)
   (display (factorial n))
   (newline))
 primes)

(define (safe-div a b)
  (call-with-current-continuation
   (lambda (k)
     (if (zero? b)
         (k 'error)
         (/ a b)))))

(when (> answer 40)
  (write `(answer ,answer ,@primes)))
(unless (null? primes)
  (display (string-append "first: " (number->string (car primes)))))
