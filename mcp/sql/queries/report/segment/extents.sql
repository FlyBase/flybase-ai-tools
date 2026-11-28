-- Source accession coordinates displayed on the recombinant construct report.
select src.uniquename as accession,
  case when loc.fmin = loc.fmax then loc.fmin else loc.fmin + 1 end as start,
  loc.fmax as end_pos, loc.strand
from feature f join featureloc loc using (feature_id)
join feature src on src.feature_id = loc.srcfeature_id
where f.uniquename = $1
order by loc.locgroup, loc.rank, loc.featureloc_id limit $2
