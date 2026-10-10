from greet import hello

# §4.4 cases 1, 3 and 7: each name is declared after its case's text,
# and used away from the end of the next row.
s1 = "😀"; e1 = 1
u1 = "😀"; w1 = (e1, "keeps the use away from the line end")
s3 = "👨‍👩‍👧"; e3 = 3
u3 = "👨‍👩‍👧"; w3 = (e3, "keeps the use away from the line end")
s7 = "é"; e7 = 7
u7 = "é"; w7 = (e7, "keeps the use away from the line end")
print(s1, u1, w1, s3, u3, w3, s7, u7, w7)
print(hello())
