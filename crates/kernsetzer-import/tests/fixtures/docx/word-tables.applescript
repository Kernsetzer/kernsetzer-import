-- word-en-tables.docx, made by Microsoft Word for Mac.
--
-- Word is driven through its AppleScript dictionary: a document with the
-- table shapes the .docx importer has to read: merged cells in both
-- directions, a two-row header with a group heading and a label over two
-- rows, banner rows, a short row (`w:gridAfter`), and, in a second table, a
-- cell of several paragraphs, a numbered list, rotated text, a nested table
-- and a footnote; then a box of one cell. The text is Jane Austen, *Pride
-- and Prejudice*, verbatim from samples/en-sample.md (public domain; sources
-- in samples/README.md). No picture in a cell: Word links a file it is
-- given by AppleScript instead of embedding it, so the file would not hold
-- the picture (the picture case is a synthetic test).
--
-- Word is sandboxed: it may only read and write inside its own container.
-- A path anywhere else (the repository, /tmp) makes Word wait for a person to
-- answer a "Grant File Access" dialog, which blocks the script. So the
-- script saves into the container and the files are copied from there:
--
--   C="$HOME/Library/Containers/com.microsoft.Word/Data"
--   osascript word-tables.applescript "$C/word-tables-build-1.docx" "$C/word-tables-build-1.pdf"
--   cp "$C/word-tables-build-1.docx" word-en-tables.docx
--
-- The second argument (optional) is a PDF to export too: the pages as Word
-- draws them, to compare with the table the importer reads.
-- Use a file name that no open Word
-- document has: saving over the name of an open document makes Word stop
-- understanding "save as" for the new one. The script closes the document it
-- made (and no other). If a "Grant File Access" sheet is open, Word does not
-- understand "close" either: the document stays open, close it by hand.
--
-- Then re-bless the golden (SETZER_BLESS=1 cargo test -p setzer-import
-- --test golden) and review the diff.
--
-- Written for Word 16.114 (Mac) with a German UI: only built-in style
-- constants are used, so the UI language does not matter. Its styles are
-- written as "berschrift1" (Heading 1) and "Listenabsatz" (List Paragraph);
-- the importer reads the style names in styles.xml ("heading 1"), not these.

on run argv
	set outPath to item 1 of argv
	set pdfPath to ""
	if (count of argv) > 1 then set pdfPath to item 2 of argv
	set lq to character id 8220 -- left double quote
	set rq to character id 8221

	with timeout of 120 seconds
		tell application "Microsoft Word"
			set d to make new document

			-- The paragraphs. Empty ones (3, 7, 9) are where the tables
			-- go; Word keeps the empty paragraph after a table, as authors do.
			set body to {"Chapter I", "Mr. Bennet replied that he had not.", "", "This was invitation enough.", "Chapter II", "Mrs. Bennet deigned not to make any reply; but unable to contain herself, began scolding one of her daughters.", "", "The girls stared at their father.", "", "Mr. Bennet was among the earliest of those who waited on Mr. Bingley."}
			set AppleScript's text item delimiters to return
			set content of text object of d to (body as text)
			set AppleScript's text item delimiters to ""
			set style of text object of paragraph 1 of d to style heading1
			set style of text object of paragraph 5 of d to style heading1
			-- The book's title and author, as the file's properties say
			-- them (Word fills the author with the user's name).
			set value of document property "Title" of d to "Pride and Prejudice"
			set value of document property "Author" of d to "Jane Austen"

			-- Table 3 first (the last in the document, so the paragraph
			-- numbers above stay right): a box of one cell.
			set p9 to text object of paragraph 9 of d
			set t3 to make new table at d with properties {text range:(create range d start (start of content of p9) end (start of content of p9)), number of rows:1, number of columns:1}
			set content of text object of (get cell from table t3 row 1 column 1) to ("It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in want of a wife.")

			-- Table 2: what a cell can hold besides one line.
			set p7 to text object of paragraph 7 of d
			set t2 to make new table at d with properties {text range:(create range d start (start of content of p7) end (start of content of p7)), number of rows:5, number of columns:2}
			my fill(t2, 1, 1, "Said")
			my fill(t2, 1, 2, "Line")
			my fill(t2, 2, 1, "Mrs. Bennet")
			my fill(t2, 2, 2, lq & "Oh! single, my dear, to be sure!" & return & "A single man of large fortune; four or five thousand a year." & rq)
			my fill(t2, 3, 1, "Mr. Bennet")
			my fill(t2, 3, 2, "Mr. Bennet replied that he had not." & return & "This was invitation enough." & return & "Mr. Bennet made no answer.")
			my fill(t2, 4, 1, "Bingley.")
			my fill(t2, 4, 2, "Mr. Bennet replied that he had not." & return)
			my fill(t2, 5, 1, "Mrs. Bennet")
			my fill(t2, 5, 2, "Mr. Bennet made no answer.")
			set heading format of row 1 of t2 to true

			-- A numbered list in a cell.
			set c32 to get cell from table t2 row 3 column 2
			apply number default (list format of text object of c32)

			-- Rotated text in a cell.
			set orientation of text object of (get cell from table t2 row 4 column 1) to text orientation upward

			-- A footnote in a cell.
			set c52 to text object of (get cell from table t2 row 5 column 2)
			set e52 to end of content of c52
			make new footnote at d with properties {text range:(create range d start (e52 - 1) end (e52 - 1))}
			set content of text object of footnote 1 of d to "This was invitation enough."

			-- A table inside a cell: after the paragraph already there.
			set c42 to text object of (get cell from table t2 row 4 column 2)
			set e42 to end of content of c42
			make new table at d with properties {text range:(create range d start (e42 - 1) end (e42 - 1)), number of rows:2, number of columns:2}
			-- The new table is found through the cell it is in (a text range
			-- inside a cell answers with the table around it).
			set tn to table 1 of (get cell from table t2 row 4 column 2)
			my fill(tn, 1, 1, lq & "What is his name?" & rq)
			my fill(tn, 1, 2, lq & "Bingley." & rq)
			my fill(tn, 2, 1, lq & "Is he married or single?" & rq)
			my fill(tn, 2, 2, lq & "Oh! single, my dear, to be sure!" & rq)

			-- Table 1: a two-row header with a group heading over two
			-- columns and a label over two rows, banner rows, a merge down
			-- a column, a short row.
			set p3 to text object of paragraph 3 of d
			set t1 to make new table at d with properties {text range:(create range d start (start of content of p3) end (start of content of p3)), number of rows:8, number of columns:4}
			my fill(t1, 1, 1, "Said")
			my fill(t1, 1, 2, "Lines")
			my fill(t1, 1, 4, "Words")
			my fill(t1, 2, 2, "Question")
			my fill(t1, 2, 3, "Answer")
			my fill(t1, 3, 1, "Chapter I")
			my fill(t1, 4, 1, "Mr. Bennet")
			my fill(t1, 4, 2, lq & "What is his name?" & rq)
			my fill(t1, 4, 3, lq & "Bingley." & rq)
			my fill(t1, 4, 4, "5")
			my fill(t1, 5, 2, lq & "Is he married or single?" & rq)
			my fill(t1, 5, 3, lq & "Oh! single, my dear, to be sure!" & rq)
			my fill(t1, 5, 4, "12")
			my fill(t1, 6, 1, "Narrator")
			my fill(t1, 6, 2, "Mr. Bennet made no answer.")
			my fill(t1, 7, 1, "Chapter II")
			my fill(t1, 8, 1, "Mr. Bennet")
			my fill(t1, 8, 2, lq & "I hope Mr. Bingley will like it Lizzy." & rq)
			my fill(t1, 8, 4, "7")
			set heading format of row 1 of t1 to true
			set heading format of row 2 of t1 to true
			-- Alignment: the numbers right (the group heading and the second
			-- banner are centred after the merges, below).
			set a to align paragraph right
			set alignment of paragraph format of text object of (get cell from table t1 row 1 column 4) to a
			set alignment of paragraph format of text object of (get cell from table t1 row 4 column 4) to a
			set alignment of paragraph format of text object of (get cell from table t1 row 5 column 4) to a
			set alignment of paragraph format of text object of (get cell from table t1 row 8 column 4) to a

			-- Merges: down first, while every row still has all its cells;
			-- then across, right to left.
			merge cell (get cell from table t1 row 1 column 1) with (get cell from table t1 row 2 column 1)
			merge cell (get cell from table t1 row 1 column 4) with (get cell from table t1 row 2 column 4)
			merge cell (get cell from table t1 row 4 column 1) with (get cell from table t1 row 5 column 1)
			merge cell (get cell from table t1 row 1 column 2) with (get cell from table t1 row 1 column 3)
			merge cell (get cell from table t1 row 3 column 1) with (get cell from table t1 row 3 column 4)
			merge cell (get cell from table t1 row 7 column 1) with (get cell from table t1 row 7 column 4)
			-- A short row: the last cell of row 6 deleted.
			delete (get cell from table t1 row 6 column 4)

			-- Centred, after the merges: merging across resets the
			-- alignment of the cell.
			set ctr to align paragraph center
			set alignment of paragraph format of text object of (get cell from table t1 row 1 column 2) to ctr
			set alignment of paragraph format of text object of (get cell from table t1 row 7 column 1) to ctr

			save as d file name outPath file format format document
			-- The document is renamed by saving it, so `d` is stale now.
			if pdfPath is not "" then save as (active document) file name pdfPath file format format PDF
			set ours to name of (active document)
			try
				close document ours saving no
			end try

		end tell
	end timeout

end run

on fill(t, r, c, txt)
	tell application "Microsoft Word"
		set content of text object of (get cell from table t row r column c) to txt
	end tell
end fill
