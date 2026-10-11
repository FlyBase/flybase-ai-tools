select a.rank, a.surname, a.givennames from pubauthor a join pub p on p.pub_id = a.pub_id where p.uniquename = $1 order by a.rank
